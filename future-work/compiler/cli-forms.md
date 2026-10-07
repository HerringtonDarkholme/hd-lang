# New Compiler: CLI Forms Against The Specification

Status: implementation audit only, 2026-10-07, at `92baf79f`; nothing in
this report is accepted behavior. It compares the forms in
[`spec/cli/command-line.md`](../../spec/cli/command-line.md) with
`compiler/crates/hd_cli/src/main.rs`.

## Method

I built `hd_cli` in release mode. Each row ran the resulting binary in a
fresh copy of `compiler/samples/data`, with a separate temporary cache.
`FILE.hd` was `compiler/samples/hello/hello.hd`; `FILE.wasm` came from the
implementation-only `build FILE.hd -o FILE.wasm` form.

The Actual column is `exit; first output line`. It prefers standard error,
then standard output, and writes `<empty>` when neither has a line. Paths
and names are normalized below; every listed form was invoked separately.

Across the 71 rows below, one matches, seven differ, and 63 are missing.

## Entry, Help, Run And Build

| Rule | Form | Expected behavior | Actual | Result |
| --- | --- | --- | --- | --- |
| `cli.repl.open.terminal` | `hd` on a terminal | Open the REPL | `2; usage:` | missing |
| `cli.stdin.program` | `hd` with piped input | Run the input as a single-file program | `2; usage:` | missing |
| `cli.command.help` | `hd help` | Print every command, exit 0 | `2; usage:` | missing |
| `cli.command.help` | `hd --help` | Print every command, exit 0 | `2; usage:` | missing |
| `cli.command.help.command` | `hd help add` | Print `add` usage and `--dev`, exit 0 | `2; usage:` | missing |
| `cli.command.help.command` | `hd add --help` | Print the same command help, exit 0 | `2; usage:` | missing |
| `cli.file.run` | `hd FILE.hd` | Run the single file | `0; 42` | match |
| `cli.profile.release.file` | `hd --release FILE.hd` | Run the file with release semantics | `2; usage:` | missing |
| `cli.wasm.run` | `hd FILE.wasm` | Run a module written by `hd build` | `2; usage:` | missing |
| `cli.run.default.one` | `hd run` | Run the package's sole executable | `2; usage:` | missing |
| `cli.run.name` | `hd run NAME` | Run the named executable or task | `2; error: NAME: not an .hd file` | differs |
| `cli.run.file` | `hd run FILE.hd` | Reject FILE and suggest `hd run` or a NAME | `0; 42` | differs |
| `cli.command.positional` | `hd run DIR` | Reject a directory as neither NAME nor FILE | `0; 3` | differs |
| `cli.profile.release` | `hd run --release` | Run the sole executable in release profile | `2; error: --release: not an .hd file` | missing |
| `cli.profile.release` | `hd run --release NAME` | Run NAME in release profile | `2; usage:` | missing |
| `cli.package.whole`, `cli.build.output` | `hd build` | Build every executable to `build/debug` | `2; usage:` | missing |
| `cli.build.output.file` | `hd build FILE.hd` | Write `build/debug/files/FILE.wasm` | `2; usage:` | missing |
| `cli.profile.release` | `hd build --release [FILE.hd]` | Build into the release output directory | `2; usage:` | missing |
| `cli.exit.hd-failure` | unknown command or flag | Reject it with status 101 | `2; usage:` | differs |
| no spec form | `hd build FILE.hd -o OUT.wasm` | Reject the unsupported `-o` form | `0; <empty>` | differs |
| command table; `cli.exit.hd-failure` | `hd parse FILE.hd` | `parse` is not a command; reject with 101 | `2; usage:` | differs |

## Check And Test

| Rule | Form | Expected behavior | Actual | Result |
| --- | --- | --- | --- | --- |
| `cli.package.whole`, `cli.check.default` | `hd check` | Check the package without test code | `2; usage:` | missing |
| `cli.file.check-test`, `cli.package.file` | `hd check FILE.hd` | Check one single-file program or package module | `2; usage:` | missing |
| `cli.check.tests` | `hd check --tests` | Also check tests and doc tests | `2; usage:` | missing |
| `cli.check.all` | `hd check --all` | Also check tests and tasks | `2; usage:` | missing |
| `cli.check.max-errors` | `hd check --max-errors 5` | Print at most five diagnostics | `2; usage:` | missing |
| `cli.check.summary-mode` | `hd check --summary` | Print counts by code and file | `2; usage:` | missing |
| `cli.package.whole`, `cli.test.package-empty` | `hd test` | Run every package test; an empty package passes | `2; usage:` | missing |
| `cli.file.check-test`, `cli.test.file-empty` | `hd test FILE.hd` | Test one file; no registered case is an error | `2; usage:` | missing |
| `cli.profile.test.release` | `hd test --release` | Keep checked semantics and use the optimized pipeline | `2; usage:` | missing |
| `cli.test.filter` | `hd test [FILE.hd] --filter PATTERN` | Run names containing PATTERN | `2; usage:` | missing |
| `cli.test.seed.flag` | `hd test --seed 42` | Give every property test base seed 42 | `2; usage:` | missing |
| `cli.test.affected` | `hd test --affected` | Run test programs with changed fingerprints | `2; usage:` | missing |
| `cli.test.doc.update` | `hd test --update` | Update failing snapshots, including doc snapshots | `2; usage:` | missing |
| `cli.command.positional` | `hd test DIR` | Reject a directory and suggest `-p` | `2; usage:` | missing |

## Documentation, Creation And Maintenance

| Rule | Form | Expected behavior | Actual | Result |
| --- | --- | --- | --- | --- |
| `cli.doc.command` | `hd doc` | Write package Markdown under `build/doc` | `2; usage:` | missing |
| `cli.doc.private` | `hd doc --private` | Include private items | `2; usage:` | missing |
| `cli.doc.files` | `hd doc --out DIR` | Write the documentation to DIR | `2; usage:` | missing |
| `cli.doc.open` | `hd doc --open` | Write the site and open its root page | `2; usage:` | missing |
| `cli.doc.name` | `hd doc NAME` | Print one item or module and write no file | `2; usage:` | missing |
| `cli.new.app` | `hd new --app PATH` | Create an application package | `2; usage:` | missing |
| `cli.new.lib` | `hd new --lib PATH` | Create a library package | `2; usage:` | missing |
| `cli.new.here.dir` | `hd new --app` or `hd new --app .` | Create the package in the working directory | `2; usage:` | missing |
| `cli.new.kind.no-terminal` | `hd new [PATH]` without a terminal | Reject and name `--app` and `--lib` | `2; usage:` | missing |
| `cli.new.vcs-none` | `hd new --app --vcs none PATH` | Create without Git metadata | `2; usage:` | missing |
| planned-section note; `cli.new.pages` | `hd new --app --pages PATH` | Currently reject the unavailable planned flag, with status 101 | `2; usage:` | differs |
| `cli.dep.add` | `hd add NAME PATH@VERSION` | Add or change a dependency | `2; usage:` | missing |
| `cli.dep.add.dev` | `hd add --dev NAME PATH@VERSION` | Add or change a development dependency | `2; usage:` | missing |
| `cli.dep.update` | `hd update` | Update every dependency on its compatibility line | `2; usage:` | missing |
| `cli.dep.update` | `hd update NAME` | Update one dependency | `2; usage:` | missing |
| `cli.dep.remove` | `hd remove NAME` | Remove one dependency | `2; usage:` | missing |
| `cli.dep.fetch` | `hd fetch` | Fetch selected versions and record missing hashes | `2; usage:` | missing |
| `cli.clean.build` | `hd clean` | Remove package build output | `2; usage:` | missing |
| `cli.clean.cache` | `hd clean --cache` | Remove fetched and compiled cache entries | `2; usage:` | missing |
| `cli.fmt.package` | `hd fmt` | Format package source, test and task files | `2; usage:` | missing |
| `cli.fmt.file` | `hd fmt FILE.hd` | Format one file | `2; usage:` | missing |
| `cli.fmt.check` | `hd fmt --check [FILE.hd]` | Report unformatted paths without writing | `2; usage:` | missing |
| `cli.fix.command` | `hd fix` | Apply safe fix-its and recheck | `2; usage:` | missing |
| `cli.cache.gc` | `hd cache gc` | Evict compiled entries to the size cap | `2; usage:` | missing |

## Shared Argument Forms

| Rule | Form | Expected behavior | Actual | Result |
| --- | --- | --- | --- | --- |
| `cli.args.separator`, `cli.args.pass` | `hd FILE.hd -- a b` | Pass `a`, `b` to the program | `2; usage:` | missing |
| `cli.args.separator`, `cli.args.pass` | `hd run NAME -- a b` | Pass `a`, `b` to the program | `2; usage:` | missing |
| `cli.args.extra-word` | `hd run NAME x` | Reject and suggest `--` | `2; usage:` | missing |
| `cli.cap.flag.commands` | `--cap TRAIT=VALUE` before `--` | Override a run, file, test or REPL grant; repeatable | `2; usage:` | missing |
| `cli.limit.heap`, `cli.limit.commands` | `--max-heap 256M` | Limit each program heap | `2; usage:` | missing |
| `cli.limit.time`, `cli.limit.commands` | `--time-limit 30s` | Limit each program's duration | `2; usage:` | missing |
| `cli.memory.cap` | `--max-memory 4G` | Limit compiler memory | `2; usage:` | missing |
| `cli.jobs` | `--jobs 1` | Use one worker thread | `2; usage:` | missing |
| `cli.workspace.select.anywhere` | `-p NAME` | Select one workspace member | `2; usage:` | missing |
| `cli.workspace.select.anywhere` | `--package NAME` | Select one workspace member | `2; usage:` | missing |
| `cli.workspace.select.repeat` | repeated `-p NAME` | Select only the named members | `2; usage:` | missing |
| `cli.json.commands` | `--format json` | Emit the command's specified JSON-lines stream | `2; usage:` | missing |

## Findings

The implementation has three accepted shapes: `hd FILE.hd`, `hd run
FILE.hd|DIR`, and `hd build FILE.hd|DIR -o OUT.wasm`. Only the first is a
specified form with the specified meaning.

The largest semantic conflict is `hd run FILE`: the implementation runs it,
while `cli.run.file` requires an error. Directory targets have the same
conflict with `cli.command.positional`. Conversely, package NAME selection
is read as a file path and fails before package discovery.

Every rejected form exits 2. `cli.exit.hd-failure` requires 101 for command
line rejection, including the intentionally removed `hd parse` command and
the currently unavailable `--pages` flag.

The current `USAGE` text accurately describes the implementation, not the
specification. It lists neither the package forms nor any specified flag,
and it advertises the implementation-only `build ... -o ...` form.
