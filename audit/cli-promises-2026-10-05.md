# Job AI: What The CLI Promises But Doesn't Do (2026-10-05)

Acted as a user with fresh `hd new --app` and `hd new --lib` packages
(under `/tmp`, not committed). Every `hd help` command and flag was run;
all generated files were read; every printed hint was followed. The full
`new → check → run → test → build` lifecycle works, error hints are
excellent (`run nosuch` names the executable; `run seed extra` suggests
`--`; wrong-command flags name the commands that accept them), and so
are `explain`, `repl` (`:type`, `:source`, `:reset`), `debug parse|hir`,
tasks, `hd build FILE`, and `--format json` on `check` and `doc`.

Broken, missing, or misleading items follow. Each gives what ran, what
happened, what the spec says, and a suggested fix. Nothing was fixed in
this job.

## 1. `hd new --pages` writes a workflow that cannot run

- Ran: `hd new --app --pages paged`, then read
  `paged/.github/workflows/docs.yml`.
- Happened: the workflow runs `hd doc --out _site`.
- Spec: `cli.new.pages.workflow` says the workflow "runs `hd doc` and
  deploys the output directory to GitHub Pages".
- Problem: `hd doc` takes only a NAME (`hd doc --out site` exits 101
  with `unknown flag --out`; CLI-DOC known failures). The generated
  workflow fails on its first `hd` step.
- Fix: implement `hd doc --out DIR` (in the `hd doc` redesign,
  orchestrator task #309) or template a command that exists.

## 2. `hd doc` is being redesigned (task #309); one line

`hd doc main` works, but no prelude or `std.` name resolves
(`println`, `std.console.println`) and neither does an imported name
(`assert_equal` in the file that uses it): always `no symbol named …`,
exit 1. The `cli.doc.name.std` rule wants `std.` names.

## 3. The host profile binds only `Console`

- Ran: a task reading `std.host.args` behind `$ Console + Args`,
  then `hd run args -- foo bar`.
- Happened: `nonhost-entry-requirement: entry point requirement 'Args'
  is not supplied by the MVP host profile`, exit 101.
  `src/checker/program.ts:223` binds only `Console` (plus test-runner
  extras).
- Spec: the Host Capabilities table (`command-line.md`, `cli.host.*`)
  promises a default profile binding `Console`, `ConsoleInput`,
  `Args`, `Env`, `Clock`, `Random`, `FsRead`, `FsWrite`.
- Problem: `hd run -- args` is accepted per `cli.args.*`, but no
  program can read the arguments (or the clock, env, or fs). Every
  documented profile trait except `Console` is a promise that fails
  at check time.
- Fix: bind the table, or cut the spec table down to the MVP profile
  and make `hd run -- extra` an error until `Args` exists.

## 4. A failing test file prints no result line

- Ran: a package with `tests/fail.hd` (one failing `assert_equal`)
  and `tests/failpkg.hd` (passing); `hd test`.
- Happened: exit 1 with `assertion-failed: boom: actual 1, expected 2`
  and `tests/failpkg.hd: 1 passed` — but no line at all for
  `tests/fail.hd`. The user must infer which file failed.
- Spec: no text-summary rule found (only the JSON objects), so this
  is a suggestion, not a violation.
- Fix: print `tests/fail.hd: 1 failed` (or the case name) alongside
  the assertion message.

## 5. `hd test --filter` and `--deny-skipped` don't exist

- Ran: `hd test --filter greeting`, `hd test --deny-skipped`.
- Happened: `unknown flag --filter` / `unknown flag --deny-skipped`,
  exit 101; `hd help test` lists neither.
- Spec: `cli.test.filter`, `cli.test.filter.none`,
  `cli.test.doc.filter`, and `cli.test.deny-skipped` require them.
- Fix: wire the flags, or drop the rules until implemented.

## 6. Workspaces are specified but absent

- Spec names workspaces 22 times, including
  `cli.new.workspace-member` (`hd new` under a workspace root adds
  the directory to the workspace manifest's `members`).
- No `src/` file handles a `[workspace]` manifest, so nesting a new
  package under anything never updates a parent.
- Fix: hide the workspace rules until implemented.
