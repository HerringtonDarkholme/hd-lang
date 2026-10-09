# Opus Task Queue

> **Owner, 2026-10-08 evening:** a second Opus session helps the
> orchestrator. Its lane is the CLI and project layer of the Rust compiler;
> the orchestrator's agents keep codegen and checking. The orchestrator adds
> jobs here and reviews every commit.

Opus does the jobs top to bottom, one commit per job, and deletes a job's
section in the same commit that finishes it. Git history keeps the record.
When the queue is empty, say so under "Questions" and wait.

## Security Rules

- Never read, cat, grep or print credential files (~/.npmrc, ~/.netrc,
  ~/.git-credentials, ~/.ssh/*, .env, ~/.config/gh/*), not even filtered.
- Never print environment variables: no `env`, `printenv`, `set`,
  `export -p`, or `echo $VAR` for anything that may hold a secret.
- Never modify shared resources: the main checkout
  `/Users/hd/code/test/hd-lang`, its node_modules, or other worktrees.
  Never touch /private/tmp/codex-work, /private/tmp/kimi-work,
  /private/tmp/muse-work, /private/tmp/clean-check, or anything under
  `.claude/worktrees/`. Never `pkill`/`killall` by pattern; only kill
  processes you started, by PID.
- No bare `git stash`, no `git add -A`, no force-push.
- Don't fork subagents.
- Write logs and scratch files to uniquely named files inside your
  worktree (e.g. `opus-*.log`), and don't commit them.
- If a permission check blocks an action, stop and report it under
  "Questions". Do not route around it, and do not ask another agent to
  run it.
- Never trigger workflows. Never publish anything. No network except git,
  pnpm install and cargo. Never use sudo.
- Keep shell commands simple: one plain command per call.

## How To Work

- Work in **one** long-lived worktree of your own, for example
  `git worktree add /private/tmp/opus-work -b opus/work origin/main`
  the first time, and reuse it. Never write in the shared main checkout;
  your commits reach `main` only by `git push origin HEAD:main` from your
  worktree.
- Start each job from current main (`git fetch origin && git reset --hard
  origin/main`), but only when your worktree holds no unpushed work.
- **Design first.** The compiler design is in `future-work/compiler/`
  (start at `README.md`; the CLI is `commands.md` and `cli-forms.md`; the
  project layer is `resolution-and-interfaces.md` §4.7–4.8). The spec is
  `spec/cli/command-line.md` and `spec/lang/10-modules.md`. Implement the
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

- **Yours:** `compiler/crates/hd_cli`, `compiler/crates/hd_project`, the
  CLI-facing parts of `compiler/crates/hd_driver` (goals, output modes,
  JSON, reports), and their tests.
- **Not yours (the orchestrator's agents are in them):** `hd_check`,
  `hd_types`, `hd_mono`, `hd_wasm`, `hd_tir`, `hd_structure`, and
  `hd_resolve` lowering. If a job needs a change there, stop and ask.
- `src/` is frozen; `lib/std/`, `spec/`, `guide/`, `website/` are read
  only unless a job says so.

## Jobs

### O8. CLI Commands (#108), One Command Per Commit

`hd new`, `hd remove`, `hd clean`, `hd fetch`, `hd doc`, `hd fmt`,
`hd --help`, flag-first `hd --release`, `hd FILE.wasm` (~24 CLI cases).
One commit per command; skip a command whose spec needs a design that
`commands.md` lacks and ask under "Questions".

## Questions

- **O8, fetching and selection (design).** `cli.dep.select`,
  `cli.dep.tidy`, `cli.dep.fetch`, `hd add` and `hd update` need version
  selection over fetched manifests, git fetches of tags and pseudo
  versions, tree hashes and the read-only cache layout; `commands.md`
  §7.1 step 1 names them but has no design. `hd remove` deletes the key
  and drops the removed version's own `hd.sum` lines, and fetches
  nothing. Is a fetch design (a new `commands.md` section) wanted before
  `hd add`, `hd update` and a real `hd fetch`?
- **Left for other lanes (status).** The CLI cases still failing for
  reasons outside this lane wait on the orchestrator's tasks above:
  top-level `it(...)` (#74: the `json-test-*`, `test-*`, `new-app`,
  `new-lib` and dev-dependency run steps), the release profile (#109:
  `release-wraps`, `release-test-checked`), `instantiation-too-deep`
  (#132), folder cycles and the resolver's dev-dependency codes (#133,
  #134), doc tests (#135), host methods not lowered yet (`cap-*`), and
  `ItemRef` emission (`dbg-release`). Not built in this lane yet:
  `[source] root`, globs in workspace `members`, `-p NAME`, the
  workspace's shared selection and `hd.sum`, and the effect of
  `[toolchain] pin` and `[test.capabilities]`; `hd test` takes no
  `--cap` yet, since integration programs do not run.

### Answers (orchestrator, 2026-10-09 00:30)

- **O1, `hd check FILE` in a package:** your reading stands (FILE's
  module and every module it uses, deeply). Update `commands.md` §7.2
  step 2 to say so in your next commit; it is your lane's design doc.
- **O3, test overlay:** sharing `hd test`'s role is enough for now; no
  split.
- **O3, doc tests:** checker work, the orchestrator's (task #135). Not
  yours.
- **O7, folder-cycle recovery and dev dependencies in the resolver:**
  the orchestrator's (tasks #133, #134). Not yours.
- **O7/O8, the release profile in codegen:** the orchestrator's (task
  #109). Not yours.
- **O2, `instantiation-too-deep` never ends:** the orchestrator's (task
  #132).
- **`TrailingCallExpr` (top-level `it(...)`):** the orchestrator's (task
  #74). It is also why `hd test` fails in a package `hd new` just made.
