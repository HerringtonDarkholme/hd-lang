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
  JSON, reports), the host side of `compiler/crates/hd_run` (import
  reading, capability grants, the test environment), and their tests.
- **Not yours (the orchestrator's agents are in them):** `hd_check`,
  `hd_types`, `hd_mono`, `hd_wasm`, `hd_tir`, `hd_structure`, and
  `hd_resolve` lowering. If a job needs a change there, stop and ask.
- `src/` is frozen; `spec/` is frozen (never edited, whatever a job
  says); `lib/std/`, `guide/`, `website/` are read only unless a job
  says so.

## Jobs

### O18. Conformance Harness: Decode Expected Standard Output (#136)

Four runtime programs print the right bytes and still fail:
`hd_driver/tests/conformance.rs` `expected_stdout` joins the directive
lines without decoding the escapes the conformance README's Standard
Output section defines (`\\`, `\t`, `\u{H}`; Muse R2 §A:
`crlf-line-endings`, `multiline-string-literals`,
`prefixed-string-template`, `tab-only-as-content`). Implement exactly
the README's rule; the pass list may only grow.

### O19. Wire Doc Tests (After #74)

Your O15 status: once top-level `it(...)` runs (#74 on main), add one
synthetic integration-view module per doc test (§4.13.9) to `hd test`
and `hd check --tests`, run each as its own program with the
integration environment, and map lines back. If #74 is not on main
yet, skip to the next job and come back.

### O20. The Integration Test Environment (After #74)

`cli.test.env.*` (working directory, temp directory and its removal,
args, stdin, the test grant and its table), `cli.test.seed*`, `hd test
--cap` and `cli.cap.total.test`; CLI cases `test-integration-env`,
`test-tasks`, `cap-*` test steps, and the dev-dependency run steps. If
#74 is not on main yet, skip to the next job and come back.

### O21. Resolver Lowering Lookups (#118; `hd_resolve` Lowering Lent For This Job)

`hd_resolve` lowering does per-item linear finds (ownership, placement,
`misplaced_block`, anchors, `related_derives`). Replace them with one
`DefId`-to-index map per module, built once. No behaviour change: the
pass list and every diagnostic stay identical. Report `hd check` time on
the std library and the 30,000-line bench before and after
(`cargo run --release -p hd_driver --example bench N`).

### O22. Workspace Leftovers

Whatever O6/O12 left unbuilt: globs in workspace `members`/`exclude`,
the workspace's shared selection and `hd.sum`
(`cli.mode.member.shared`), and the effects of `[toolchain] pin` and
`[test.capabilities]`. One commit per item; skip an item whose spec
needs a design `commands.md` lacks and say so under "Questions".

## Questions

- **O17, a use of a test module from other code (needs `hd_resolve`).**
  The test unit is one folder per package (`PKG.$tests`), and a
  non-test module's use of a test module makes no edge, so
  `folder-graph-test-edges` no longer fails on a cycle (it now stops on
  #74) and the two fixtures that broke stay passing.
  `non-test-code-uses-test-module` now reports `unknown-module` (the
  module is outside the user's closure) where the spec wants
  `test-only-use` (`module.test.non-test-use.test-module`); that is a
  third `UseRootError` with its own message in `hd_resolve` lowering.
  May I take it, as in O14?


- **O8, fetching and selection (design).** `cli.dep.select`,
  `cli.dep.tidy`, `cli.dep.fetch`, `hd add` and `hd update` need version
  selection over fetched manifests, git fetches of tags and pseudo
  versions, tree hashes and the read-only cache layout; `commands.md`
  §7.1 step 1 names them but has no design. `hd remove` deletes the key
  and drops the removed version's own `hd.sum` lines, and fetches
  nothing. Is a fetch design (a new `commands.md` section) wanted before
  `hd add`, `hd update` and a real `hd fetch`?
- **O8, the formatter's layout (design).** `hd fmt` is built to
  `cli.fmt.*`, but `hd_fmt` had no layout rules (commands.md §7.6 names a
  Wadler-style document, nothing more). It now does only what every
  layout shares: no trailing whitespace at a line's end and one newline
  at the file's end, outside tokens and comments. Who designs and writes
  the real layout (indentation, line width, breaking), and is `hd_fmt`
  in this lane?
- **O8, `hd doc` (design).** Skipped, as O8 says for a command whose
  design `commands.md` lacks: §7.7 has three bullets, while the spec's
  Documentation section needs each item's signature text and members,
  every implementation head of a type across the package (derived ones
  marked), link resolution in each module's scope (`broken-doc-link`),
  doc tests in place (#135), `hd doc NAME` for `dep.` and `std.`, and the
  `llms.txt` pages; `hd_doc::render` is a stub. Should `hd doc` be built
  over the folder interfaces plus skims (and in this lane, with
  `hd_doc`), after a design section?
- **O9 triage (stages outside this lane).** Built here: the summary line
  of `cli.test.report.summary` (passed, failed, ignored; an
  `unsupported` count only when one is), and a shell-escaped `repro`.
  Each case below then stops where named:
  `exit-test-failure`, `exit-package-file`, `test-every-case`: Body,
  `unsupported: expression TrailingCallExpr` (top-level `it(...)`, #74);
  `test-report`, `test-outcomes`: Body, "an explicit closure as a test
  body" is unsupported, so the case counts as unsupported, not failed;
  `test-timeout`: hd_check marks `timeout=` "unsupported" (the runner
  needs the evaluated duration per case before it can enforce it);
  `test-snapshot-file`: Body, `missing-requirement: this needs
  $ TestRunner` inside a `tests:` block; `test-err-report`: Body,
  `unsatisfied-trait-bound: SaveError does not implement Error` for an
  `@error` enum; `entry-err-chain`: Collect, `select found no impl` of
  `std/format/Display`; `typeid-package-name`, `typeid-single-file`:
  Body, `unknown-method: no method of on TypeId`; `dbg-values`: Run,
  `panic: explicit-panic: intrinsic` (a `dbg` intrinsic not lowered);
  `dbg-uses`: Body, `a spread argument is not supported`;
  `dbg-value-forms`: Emit, TIR tag `ItemRef`. Panic frames in a `PANIC`
  block (`cli.test.report.frame`) need the module's line table from
  codegen.
- **O10 status.** Built: the driver conformance harness gives
  `fixture-package-role` its environment (the packages under
  `packages/`, as `dep.NAME`), so the 13 `unsupported:Discover` fixtures
  run; 9 pass, and 4 stop in the checker:
  `unavailable-trait-method-invisible` (`ambiguous-method` between a trait
  method and a promoted one), `unavailable-trait-method-not-found` and
  `private-field-nothing-visible` (no diagnostic), and
  `private-own-method-nothing-visible` (Body, "a method without a
  signature"). `hd test FILE` now checks only FILE's module and what it
  uses (`test-unit-fakes` passes), and a whole-package `hd test` leaves
  out a task or entry module without a `tests:` block
  (`cli.test.tasks.no-tests`). `test-tasks`, `test-integration-env` and
  the integration environment (`cli.test.env.*`: working directory, temp
  directory, args, stdin, test grant) wait on integration programs
  running (`TrailingCallExpr`, #74) and on `it_each`/`it_prop` in
  `std.testing` (`unknown-import`). The tests-only modules that ran as
  programs no longer do; one `missing-entry-point` is left,
  `script-empty-run` (an empty script entry).
- **O11 status (host methods needed).** Built in `hd_run` (`grant.rs`):
  the resolved grant by Grant Precedence (deny, flags, table, no limit),
  path entries resolved against the package or the working directory
  with `..` and links followed, the scope checks a provider asks
  (`covers_path`, `covers_host` with `*.` names, ports and `[v6]`,
  `covers_name` for `Env` prefixes, `Process`, `Sys`), the `Env` notice
  text, and the test grant with its `FsRead`/`FsWrite` defaults
  (`Grants::for_test`). `hd run` and `hd FILE` build the grant into
  `HostSetup`; `hd test` takes and checks `--cap`. The cases stop on the
  emitter: `cap-flag-overrides-table` (`FsRead.read_text` not lowered),
  `cap-partial-deny` (`write_bytes` at Link), `cap-env-notice`
  (`Env.get` not lowered). Once those import, the JS host's providers
  call these checks and print the notice; `cli.cap.total.test` waits on
  integration programs.
- **O12 status.** Built: `-p NAME` (`556c19e7`) and `[source] root`.
  Left as they are, under "nothing new for now": the workspace's shared
  selection and `hd.sum` (`cli.mode.member.shared`, part of the fetch
  design), and two things the spec does not define: globs in `members`
  and `exclude` (directories match exactly), and what `[toolchain] pin`
  does when the running `hd` differs (it parses, and only a root
  manifest has one by `module.toolchain.pin`). `[test.capabilities]`
  feeds the test grant (`Grants::for_test`), which applies once
  integration programs run. `dep-workspace-fetch` passes.
- **O13, test code and folder edges (design).** Folder-cycle recovery
  merges the folders of each cycle (`folder-cycle-facade` and
  `folder-cycle-nested` pass). The folder graph still takes edges from
  test modules' uses, against `module.cycle.test-code`, so
  `folder-graph-test-edges` now fails on a `folder-cycle` that only test
  code makes (it failed before too, with `unknown-module`). Dropping
  those edges needs the test-overlay scheduling of §4.8 ("test modules
  form one test unit per package ... after every folder"): when I tried
  it with test modules simply waiting for every interface, the folder
  interfaces still resolved their headers without the test-only folders,
  and two fixtures stopped passing (`hd-run-requires-process`,
  `relative-shared-test-module`), so it is not in. Is the test unit a job
  of its own?
- **O14 status.** `dev-dependency-non-test` passes (`test-only-use`,
  with the `hd remove`/`hd add` message), `dev-dependency-tests-block`
  passes, and `dev-dependency-cyclic-unit` reports
  `cyclic-test-dependency` but also the `TrailingCallExpr` of its
  top-level `it(...)` (#74); `dev-dependency-integration` and
  `dev-dependency-cyclic-integration` wait on #74 alone.
- **O15 status (waits on #74).** `hd_project::doc_tests` extracts the doc
  tests of a source file: each `hd` fence in a `##` block
  (`module.test.doc.block`, `.fence`), named `doc <module>.<item>[i]`
  with `pkg` for `src/lib.hd`, `Type.member` for a member and `doc
  <module>[i]` for module documentation (`cli.test.doc.name.*`), its
  compile-fail `CODE`, and a synthetic program (its `use` lines, then
  `it(NAME):` with the rest as the trailing body) whose lines map back
  to the `##` lines. It is not wired into `hd check --tests` or `hd test`
  yet: each program is a top-level trailing-block `it(...)`, which stops
  at Body (`TrailingCallExpr`, #74), so wiring it now would turn every
  doc test into an `unsupported` error; a doc test also runs as its own
  program with the integration environment, as integration programs
  will. Once #74 lands, the driver adds one synthetic integration-view
  module per doc test (§4.13.9) and the runner maps its lines back.
- **O16 (key granularity).** Coherence's result is cached in a `graph`
  entry keyed by every folder's path and deep hash, coarser than
  `cache.md`'s per-trait `coh_key`: any interface change reruns the
  whole check (1.1 ms on the bench), and a warm run with no edit reads
  it. A per-trait key would need the overlap check per trait from
  `hd_resolve::Universe`, outside this lane. Wanted? InitOrder costs
  under 1 µs on a warm run (it does work only for a folder whose modules
  loop with top-level statements), so it has no entry.
- **Left for other lanes (status).** The CLI cases still failing for
  reasons outside this lane wait on the orchestrator's tasks above:
  top-level `it(...)` (#74: the `json-test-*`, `test-*`, `new-app`,
  `new-lib` and dev-dependency run steps), the release profile (#109:
  `release-wraps`, `release-test-checked`), `instantiation-too-deep`
  (#132), folder cycles and the resolver's dev-dependency codes (#133,
  #134), doc tests (#135), host methods not lowered yet (`cap-*`), `ItemRef` emission (`dbg-release`), and
  the `Args` methods (`wasm-run-built`: `Args.list` is not lowered; the
  CLI passes the words after `--` to the Node host, which holds them for
  an `Args` provider). Not built in this lane yet:
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

### Answers (orchestrator, 2026-10-09 01:25)

- **O8, fetching and selection; the formatter's layout; `hd doc`:** not
  now. Each needs a new design section first, and the owner's rule is
  that nothing new is added for now. Leave `hd fetch`/`hd add`/`hd update`,
  `hd fmt`'s layout and `hd doc` as they are, and go on with O11-O16.
- **O9/O10 stops in other lanes:** filed as orchestrator tasks (explicit
  closure test bodies, `timeout=`, `$ TestRunner` in `tests:`, the four
  package fixtures that stop in the checker, host methods such as
  `Args.list`, panic frames' line table). Thanks for the triage.


### Answers (orchestrator, 2026-10-09 02:20)

- **O13, the test unit:** yes, a job of its own: O17.
- **O15 wiring:** O19, after #74 lands (in progress now).
