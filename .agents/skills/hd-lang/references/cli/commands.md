# The `hd` Command

`hd help` lists every command, and `hd help COMMAND` one command's flags.
Put flags after the command word: `hd check --format json FILE.hd`.

## Install

```sh
cargo install hd-lang     # needs Rust 1.97 or newer; puts `hd` in ~/.cargo/bin
```

## One file or a package

- **A single file** runs anywhere: `hd FILE.hd`. It may import only `std`.
- **A package** is a folder with an `hd.toml`
  ([packages.md](packages.md)). Inside it, commands take no file and act
  on the whole package: `hd check`, `hd test`, `hd run`.

`hd run`, `hd build`, `hd doc`, `hd check --fix` and `hd lint --fix` work
only in a package.

## Everyday commands

| Command | Does |
| --- | --- |
| `hd FILE.hd [-- ARGS]` | run one file |
| `hd run [NAME] [-- ARGS]` | run the package's program, or the executable or task `NAME` |
| `hd check [FILE.hd]` | type-check without running |
| `hd check --tests` | also check test code; `--all` adds tasks too |
| `hd check --fix` | apply the safe fix-its to the package |
| `hd test [FILE.hd]` | run the tests of the package, or of one file |
| `hd test --filter TEXT` | only the cases whose name contains `TEXT` |
| `hd test --update` | rewrite failing snapshots |
| `hd test --affected` | only the test programs that changed since they last passed |
| `hd fmt [FILE.hd]` | format in place: one fixed style, four-space indentation, 100 columns |
| `hd fmt --check` | write nothing; list the files that would change |
| `hd lint [FILE.hd]` | optional advice on code that already checks |
| `hd build [--release]` | write `build/debug/NAME.wasm` (or `build/release/`) |
| `hd doc NAME` | print the docs of one item or module, as `hd doc std.text.split` |
| `hd new PATH --app` / `--lib` | create a package |
| `hd add TARGET` | add a dependency |
| `hd` | an interactive session on a terminal; piped input runs as a program |

## The loop for an agent

```sh
hd check --format json src/cart.hd      # or: hd check --format json --tests
hd test --format json                   # one JSON line per case
hd fmt && hd lint                       # before finishing
```

1. Fix the first error first; later ones are often follow-ons.
2. After two failed attempts at one error, look the item up with `hd doc`
   instead of guessing a third time.
3. `hd check` in a package skips test code: add `--tests` while working.

## Useful flags

| Flag | On | Does |
| --- | --- | --- |
| `--format json` | every command that reports diagnostics | one JSON object per line ([output.md](output.md)) |
| `--max-errors N` | `check`, `lint` | stop printing after the Nth error |
| `--summary` | `check`, `lint` | counts per file and code instead of each diagnostic |
| `--release` | `hd FILE`, `run`, `build` | optimized build; integer overflow wraps instead of panicking |
| `--time-limit 30s` | `hd FILE`, `run`, `test` | stop a runaway program with a `time-limit` panic |
| `--cap NAME=VALUE` | `hd FILE`, `run`, `test` | grant or deny a capability for one run, as `--cap FsRead=false` |
| `--seed N` | `test` | repeat a property test run |
| `-p NAME` | in a workspace | act on one member package |
| `--allow CODE`, `--warn CODE`, `--deny CODE` | `lint` | set one lint's level |

Program arguments always go after `--`: `hd run -- input.txt --verbose`.

## Things that trip people

- `hd run FILE.hd` is an error: `hd run` runs a package. For one file,
  write `hd FILE.hd`.
- `hd fmt` takes at most one file. Format several with one command each,
  or run `hd fmt` with no file in the package.
- `hd fmt` leaves a file with a syntax error untouched.
- `hd check`, `hd build` and `hd test` report errors only. Advice such as
  `redundant-let-mut` comes from `hd lint`.
- `hd doc` outside a package is an error. Any folder with an `hd.toml`
  will do.
- `hd build` needs a package, and writes nothing for a library.
