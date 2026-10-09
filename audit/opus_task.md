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
- **Never edit spec text or conformance fixtures.** A fixture you believe
  is wrong, or a diagnostic code or message you would change, goes under
  "Questions" (fixtures) or into `audit/compiler/diagnostic-notes.md`
  (messages, codes, positions; owner rule).
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

### O1. `hd check --format json` Gaps (#105)

About 22 CLI cases fail on `hd check --format json`: no JSON in some
modes, exit 0 where the spec says 101, an extra `unknown-module` line.
Spec: `spec/cli/command-line.md` `cli.json.*` and the check output rules
(`cli.check.report.*`, `cli.check.summary-mode.*`). Find the cases in the
CLI conformance run (`spec/conformance/cli-cases.tsv`, `cli/*/expect.txt`).

### O2. `--format json` On `hd run`, `hd build`, `hd test`, `hd FILE` (#104)

About 14 CLI cases. Same `cli.json.*` rules, including
`cli.json.diagnostic.no-position` (a diagnostic with no position prints a
null position).

### O3. `hd check --tests` And `--all` (#73)

`cli.check.tests`, `cli.check.tests.doc`, `cli.check.all`: the flags
don't exist yet. Test overlays per `checking-and-tir.md` §4.13.9 ("Test
overlay"). If the overlay task itself is missing in `hd_driver`'s
scheduling, build it there (it is CLI-facing driver work).

### O4. Test And Task Modules Have No Module Path (#114)

Owner, 2026-10-08: as in Rust, integration test programs, shared test
modules and tasks have no module path (`module.test.integration.no-path`,
`cli.task.no-path`). `hd_project` gives them internal `$tests`/`$tasks`
segments; keep those internal and drop them from everything a user sees
(diagnostics, test names, JSON, `hd test` output), naming them by file.

### O5. `denied-capability` Refusal (#86)

`hd run`/`hd test` refuse a totally denied capability with
`denied-capability` (`cli.cap.total.refuse`), text and JSON (null
position). CLI case `cap-total-deny` (its JSON step).

### O6. Manifest Sections (#107)

`hd.toml` `[capabilities]`, `[workspace]`, `[executable]`, `[profile]`,
`[test]` in project discovery (~7 CLI cases).

### O7. Folder Cycles, Dev-Dependencies, Executables Exclude Tests (#111)

Folder-cycle recovery (resolution-and-interfaces.md §4.8 rule 5),
`[dev-dependencies]` loading with `test-only-use`, and `hd run` leaving
`tests/` out of the executable program.

### O8. CLI Commands (#108), One Command Per Commit

`hd new`, `hd remove`, `hd clean`, `hd fetch`, `hd doc`, `hd fmt`,
`hd --help`, flag-first `hd --release`, `hd FILE.wasm` (~24 CLI cases).
One commit per command; skip a command whose spec needs a design that
`commands.md` lacks and ask under "Questions".

## Questions

(none)
