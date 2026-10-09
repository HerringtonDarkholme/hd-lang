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

- **O1, `hd check FILE` in a package (design).** `commands.md` §7.2
  says "Check only FILE's module", but `cli/json-file-location` expects
  the `type-mismatch` in `src/util.hd` when checking `src/main.hd`, which
  uses it (`cli.package.file`: "linked with the rest of the package"),
  while `cli/check-summary` expects siblings that FILE does not use to
  stay silent. I took the reading that fits both: FILE's module and every
  module it uses, deeply. §7.2 step 2 could say so.
- **O1 left for later jobs.** The other `hd check --format json` cases
  wait on `[[executable]]` and `[workspace]` (O6: `exe-*`,
  `json-diagnostic-fixes`, `manifest-unknown-key`, `member-unlisted`,
  `ambiguous-import`), dev-dependency loading (O7:
  `dev-dependency-non-test`, whose `unknown-module` line becomes
  `test-only-use`), `hd remove` (O8: `dep-missing-sum`, whose check step
  now passes), and `hd build --format json` (O2:
  `build-instantiation-too-deep`).
- **O2 left for other lanes.** `hd run`, `hd build`, `hd test` and
  `hd FILE` take `--format json` now. Their remaining JSON cases fail
  outside the CLI: `json-test-pass`, `json-test-fail`,
  `json-test-ignored`, `json-test-order`, `test-tasks` and
  `dev-dependency-tests-block` stop on `unsupported: stage Body:
  expression TrailingCallExpr` in a top-level integration test `it(...)`
  (hd_check); `test-outcomes` on "an explicit closure as a test body";
  `dbg-release` on emitting TIR tag `ItemRef` (hd_wasm). In
  `build-instantiation-too-deep`, `hd build` never ends (still running
  after 60 s), so monomorphization has no `instantiation-too-deep` limit
  yet (hd_mono). `cap-total-deny` waits on `[capabilities]` (O5, O6).
- **O3, the test overlay (design).** `checking-and-tir.md` §4.13.9
  has a `TestOverlay(m)` task, cached as `check-test`; in `hd_driver` it
  is still a stub, and `hd test` checks `tests:` blocks inside `Body(m)`
  under the check role "test". `hd check --tests` now reuses that path
  (`Goal::CheckTests`), so it shares `hd test`'s entries, not the plain
  check's. Splitting it into its own task would move the registrations
  out of the check entry, which `hd test`'s collection reads. Is that
  split wanted now, or is sharing `hd test`'s role enough?
- **O3, doc tests (`cli.check.tests.doc`).** Nothing extracts doc tests
  from `##` blocks yet, in `hd test` or anywhere else, so `hd check
  --tests` checks none. Extraction is checker work (synthetic files
  whose spans map back, §4.13.9). Should it be a job of its own?
- **O5 scope.** `hd run` and `hd FILE` read the built module's import
  list (a small import-section reader in `hd_run`, which `hd FILE.wasm`
  can reuse) and refuse a totally denied need with `denied-capability`.
  `hd test` does not take `--cap` yet: its unit-test program gets no host
  provider, and integration test programs, whose refusal
  `cli.cap.total.test` covers, do not run yet. The other `cap-*` cases
  stop on host methods not lowered yet (`FsRead.read_text`, `Env.get`,
  `write_bytes`) or on `hd FILE.wasm` (O8).
- **O6 left out.** `[source] root` is still "not implemented" (it moves
  the source root, which discovery assumes is `src`). `members` and
  `exclude` match directories exactly, with no globs. `-p NAME`
  (`cli.workspace.select.*`) and the workspace's shared selection and
  `hd.sum` (`cli.mode.member.shared`) are not built; dependency
  selection itself is still local. `[toolchain] pin` and
  `[test.capabilities]` parse but have no effect yet. The samples under
  `compiler/samples` lost their `version = "0.1.0"` line, which is now an
  `unknown-manifest-key` warning.
- **O7, folder-cycle recovery (needs `hd_resolve`).** §4.8 rule 5
  resolves a cyclic SCC's folders together, as one folder. In
  `hd_driver`, `FolderIface(F)` builds one folder's interface through
  `hd_resolve::Cx { folder, .. }` and needs every other folder of its
  closure built first, so a cycle leaves a member blocked and its uses
  cascade (`folder-cycle-facade`: `unknown-module` after
  `folder-cycle`). Recovery means an interface over several folders in
  `hd_resolve`'s interface lowering, which is not my lane. Who takes it?
- **O7, dev dependencies (needs `hd_resolve`).** The root's
  `[dev-dependencies]` now load, and test modules, integration tests and
  tasks see them (`hd_project::ModuleTable::use_roots`); library code
  does not, and a unit test module does not see one that depends back on
  the package. Two parts need the resolver: the `test-only-use` and
  `cyclic-test-dependency` codes (a new `UseRootError` arm in
  `hd_resolve` lowering), and dev dependencies inside a library module's
  `tests:` block, whose uses `use_decls` joins to the module's scope
  (`dev-dependency-tests-block`). The run-time cases also need top-level
  `it(...)` in test modules (`TrailingCallExpr`, hd_check).
- **O7, `hd run --release`.** `hd run`, `hd build` and `hd FILE` now
  leave test code and other tasks out of the program, so
  `release-wraps` gets to running; it then panics with
  `integer-overflow`, since `--release` does not select the wrapping
  profile in codegen yet.
