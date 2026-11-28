---
name: hd-lang
description: Write, check, test and run hd-lang (hd) code - a statically typed, indentation-based language that compiles to WebAssembly, built for code that AI writes and humans review. Covers the hd way of designing a program (effects declared after `$` and provided with `$.with`, derives and your own derive templates, `require`/`ensure` contracts, `@error` enums, `!` suspension with `join!`/`retry`, property and model-based tests, minimal capability grants), a one-page core language, the standard library by job, the `hd` command and its JSON diagnostics, complete tested programs by scenario (CLI tool, stdin filter, text records, configuration, files, HTTP, processes, concurrency, shared state, a parser and evaluator, porting, performance), and a lookup from each diagnostic code to its fix. Use before writing or editing any `.hd` file or `hd.toml` package, and whenever running the `hd` command.
---

# Write hd-lang Code

hd is statically typed and indentation-based, and compiles to WebAssembly.
It reads like Python and types like Rust, and it is neither: code guessed
from either language does not check. Every hd example in this skill is
checked against the compiler. When the skill and `hd` disagree, `hd` is
right.

## Setup

```sh
cargo install --locked --git https://github.com/HerringtonDarkholme/hd-lang hd-lang
hd help                  # the commands; `hd help COMMAND` for one command's flags
```

## Steps

1. **Read [language.md](language.md)** before writing hd. It is one page
   and covers the everyday language and the rules that surprise.
2. **Pick the design** from [The hd Way](#the-hd-way). When the task
   matches a [scenario](#scenarios), start from that page: it is a
   complete, tested program to adapt.
3. **Write, then check after every edit**: `hd check FILE.hd`, or
   `hd check --tests` in a package. Fix the first error first; look its
   code up in [diagnostics](reference/diagnostics.md)
   ([the loop](cli.md#the-loop)).
4. **Test**: `hd test FILE.hd`, or `hd test` in a package. A failure
   prints a `repro:` line that reruns just that case.
5. **Finish**: `hd fmt`, then `hd lint`.

Two tries at most per error. Then find out instead of guessing: a piped
probe, the [std reference](reference/std/README.md), or `hd doc`
([explore](reference/explore.md)).

## What hd Is

- **Values, lent for a call.** Structs, lists, maps and strings are values:
  assigning or passing one copies it. `&x` / `&mut x` lend a variable to
  one call; `box(value)` makes a shared object. No garbage collector, no
  null ([references](reference/references.md)).
- **Effects are in the signature.** What a function needs from outside is
  its row after `$`: `fn save() -> void $ FsWrite`. Callers provide it,
  tests swap in fakes, and a package grants only what it lists
  ([effects](design/effects.md), [capabilities](design/capabilities.md)).
- **Waiting is explicit.** A function that waits is named and called with
  `!`; `join!`, `race!` and `std.task` policies compose waits
  ([concurrency](design/concurrency.md)).
- **Errors are values.** `T?`, `Result[T, E]`, `?`, and `@error` enums
  ([errors](design/errors.md)).
- **Routine code is derived.** `@derive` writes equality, JSON, CLI
  parsers, validation and test data from a type's fields, and a template
  makes your own traits derivable ([derive](design/derive.md)).
- **Tests live with the code**, in a `tests:` block, with fakes for every
  effect and generated inputs ([testing](design/testing.md)).

## The hd Way

hd has a feature for each of these situations. Use it, rather than the
loop or `if` another language would need.

| When | Write |
| --- | --- |
| code reaches a database, service, clock or file | a trait for it, required with `$ Trait` and reached with `$.use(Trait)`; `main` provides the real one with `$.with`, tests a fake ([effects](design/effects.md)) |
| you declare an error enum | `@error` on it, with an `@error("...")` message per variant, even next to `@derive(Eq, Debug)` ([errors](design/errors.md)) |
| a call should be retried, time out or race a fallback | a `std.task` policy, `retry(Backoff { ... })` with `apply!`, not a hand loop ([concurrency](design/concurrency.md)) |
| several calls should wait together | `join!`, or `map_join!` with a limit |
| you'd write `==`, JSON, options parsing or validation by hand | `@derive` it, configured with field facts ([derive](design/derive.md)) |
| your own trait needs the same per-field `impl` on several types | a derive template, so `@derive(YourTrait)` works ([derive](design/derive.md#a-template)) |
| a call has rules the caller must keep, or a result has a guarantee | `require` / `ensure` ([contracts](design/contracts.md)) |
| something round-trips or holds for every input (encode then decode, sort, a cache against a model) | a property test with `it_prop`, beside the example tests ([testing](design/testing.md)) |
| several examples share one assertion | one `it_each` table |
| a package needs the outside world | only the capabilities it uses, scoped where it can be (`FsRead = ["data/"]`) ([capabilities](design/capabilities.md)) |
| an API lists a struct's fields | `pub` on each field: fields are private even in a `pub struct` |

## Where To Look

| For | Read |
| --- | --- |
| the everyday language, and what differs from Python, Rust, Go | [language.md](language.md) |
| a std function for a job: text, collections, files, time, JSON, ... | [std.md](std.md); every item, one file per module: [reference/std/](reference/std/README.md) |
| the `hd` command, its flags, and reading its output | [cli.md](cli.md) |
| a diagnostic code | [diagnostics.md](reference/diagnostics.md) |
| something hd can't do yet | [limitations.md](reference/limitations.md) |
| anything else | [explore.md](reference/explore.md) |

### Design

| Page | Covers |
| --- | --- |
| [effects.md](design/effects.md) | rows after `$`, your own effect traits, `$.use`, `$.with` providers, `$ R` callbacks |
| [capabilities.md](design/capabilities.md) | `[capabilities]` grants and scopes, `--cap`, sandboxing code, `@host` plugins |
| [derive.md](design/derive.md) | built-in derives and field facts; writing your own derive template |
| [contracts.md](design/contracts.md) | `require` and `ensure` |
| [errors.md](design/errors.md) | `T?`, `Result`, `@error` enums, `@from`, context, exit codes, panics |
| [testing.md](design/testing.md) | `tests:`, `it_each`, `it_prop`, model-based tests, std fakes, integration tests, snapshots |
| [concurrency.md](design/concurrency.md) | `!`, `join!`, `race!`, `map_join!`, `std.task` policies, channels, threads |

### Scenarios

Each is one task as a complete, tested program, then its decisions and
pitfalls.

| To | Read |
| --- | --- |
| build a command-line tool: options, `--help`, exit status | [cli-tool.md](scenarios/cli-tool.md) |
| read standard input and transform it | [stdin-filter.md](scenarios/stdin-filter.md) |
| parse a text or CSV file into typed records | [text-records.md](scenarios/text-records.md) |
| load configuration from JSON and the environment, and validate it | [configuration.md](scenarios/configuration.md) |
| list, read, write and move files | [files-and-directories.md](scenarios/files-and-directories.md) |
| call an HTTP API and decode JSON | [http-api.md](scenarios/http-api.md) |
| run another program and read its output | [run-commands.md](scenarios/run-commands.md) |
| do several waits at once, with timeouts and retries | [concurrent-work.md](scenarios/concurrent-work.md) |
| share and update state: `&mut`, `box`, or a provider | [shared-state.md](scenarios/shared-state.md) |
| model states, transitions and rules | [domain-model.md](scenarios/domain-model.md) |
| read a small language: lexer, parser, tree, evaluator, errors with positions | [parser.md](scenarios/parser.md) |
| start a project and split it into modules | [project-layout.md](scenarios/project-layout.md) |
| port code from Go, Python, Rust or TypeScript | [porting.md](scenarios/porting.md) |
| make a program faster | [performance.md](scenarios/performance.md) |

### Reference

| Page | Covers |
| --- | --- |
| [traits.md](reference/traits.md) | traits, generics and bounds, `dyn`, operators, conversions |
| [references.md](reference/references.md) | values and copies, `&T` / `&mut T`, `box` and `*T`, what may escape |
| [packages.md](reference/packages.md) | `hd new`, `hd.toml`, executables, tasks, dependencies, workspaces |
