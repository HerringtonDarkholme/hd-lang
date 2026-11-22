---
name: hd-lang
description: Write, check, test and run hd-lang (hd) code - a statically typed, indentation-based language that compiles to WebAssembly. Gives the edit-check loop for the `hd` command, the rules that differ from Python and Rust, and an index of focused references - how-to pages by scenario (a command-line tool, files, JSON config, an HTTP API, concurrency, shared state, project layout), syntax by topic, the standard library, testing, packages, and the diagnostics agents hit most with their fixes. Use before writing or editing any `.hd` file or `hd.toml` package, and whenever running the `hd` command.
---

# Write hd-lang Code

hd is statically typed and indentation-based, and compiles to WebAssembly.
It reads like Python and types like Rust, and it is neither: code guessed
from either language does not check. This file holds the workflow and the
rules that surprise. Each topic has its own reference file, listed under
[References](#references). Every `hd` example in this skill is checked
against the compiler.

## When to Use

- Writing or editing a `.hd` file, or a package with an `hd.toml`.
- Running `hd`: checking, testing, formatting, building, adding dependencies.
- Reading an `hd` diagnostic and deciding what to change.

## Setup

```sh
cargo install hd-lang    # installs the `hd` command; needs Rust 1.97 or newer
hd help                  # lists the commands; `hd help COMMAND` lists one command's flags
```

hd is experimental and still changes. When this skill and the compiler
disagree, the compiler is right: read its message, then `hd help` or
`hd doc`.

## Steps

1. **Read** before writing. When the task matches a scenario under
   [How to](#how-to-by-scenario), start from that page: it is a complete,
   tested program to adapt. Otherwise open the page for the topic.
2. **Write** the code.
3. **Check**: `hd check --format json FILE.hd`, or
   `hd check --format json --tests` inside a package. Each output line is
   one diagnostic with `code`, `line`, `column`, `message` and `fixes`. Fix
   the first error first; later ones are often follow-ons.
4. **Look up the code** in the [mistakes](#mistakes-by-diagnostic-code)
   files when the message alone does not give the fix.
5. **Test**: `hd test FILE.hd`, or `hd test` in a package. A failure prints
   a `repro:` line that reruns that one case.
6. **Run**: `hd FILE.hd`, or `hd run` in a package. Program arguments go
   after `--`.
7. **Finish**: `hd fmt FILE.hd` (one canonical style, no settings), then
   `hd lint FILE.hd` for optional advice.

Two tries at most per error. When a fix does not take, stop guessing: in a
package, `hd doc std.text.split` prints an item's signature and
`hd doc std.text` a whole module.

## One Program

The shape of an hd file: imports, types, functions, an entry point, tests.

```hd
use std.cmp.max

struct Order:
    id: string
    cents: i32
    tags: List[string] = []

@error
enum OrderError:
    @error("order $id has no price")
    Unpriced(id: string)

fn checked(order: Order) -> Result[i32, OrderError]:
    if order.cents <= 0:
        return .Err(.Unpriced(order.id))
    .Ok(order.cents)

fn largest(orders: List[Order]) -> Result[i32, OrderError]:
    let best = +0                        # `let` can be reassigned; `:=` cannot
    for order in orders:
        best = max(best, checked(order)?)    # `?` returns the `.Err` early
    .Ok(best)

pub fn main() -> void $ Console:         # `$ Console`: this function may print
    orders := [Order { id: "A-1", cents: 1200 }, Order { id: "A-2", cents: 450, tags: ["gift"] }]
    match largest(orders):
        .Ok(cents) => println("largest: $cents cents")
        .Err(error) => println("error: $error")

tests:
    it("an unpriced order is an error"):
        result := largest([Order { id: "B-1", cents: 0 }])
        assert_equal(result.is_err(), true, reason="B-1 has no price")
```

```output
largest: 1200 cents
```

## Rules That Surprise

Syntax:

- Blocks are `header:` plus indentation. No braces, no `end`, no semicolons.
- `!`, `&&`, `||`, `else if`, `while true:`. There is no `not`, `and`,
  `or`, `elif` or `loop`.
- `"$name"` and `"${expr}"` interpolate; `"{x}"` prints the braces.
- A record is built with `Item { name: "a" }`, never `Item(name: "a")`.
  The spread is `...base`, with three dots.
- Tuple fields are `pair._0` and `pair._1`.
- A `match` arm is `pattern => expression` or an indented body, never
  `{ ... }`. One pattern per arm: there is no `A | B` pattern.
- `use` goes at the top of the file, never inside a function.
- An expression that continues on the next line needs parentheses around
  it, unless the next line starts with `.` (a method chain) or `|>`.

Types and values:

- An integer literal takes its type from its uses. With no use that decides,
  it is `usize`, or `i32` when written with a sign, as `+5`. `len()` and
  indices are `usize`, and signed and unsigned integers do not mix: convert
  with `i32(n)` or `usize(n)`. Nothing converts implicitly.
- `:=` never reassigns. `let x = ...` reassigns. `let mut x = ...` is for a
  list, map or struct changed in place (`xs.push(v)`, `p.x = 1`).
- A struct, list, map and string are values: assigning or passing one
  copies it. `&mut x` lends a variable to one call; `box(value)` makes a
  shared object.
- There is no `null` and no `unwrap()`: use `match`, `?`,
  `unwrap_or(fallback)` or `expect("why")`.
- An unused `Result` or optional is an error: discard it with `_ := expr`.

Requirements:

- Everything a function needs from outside is in its row after `$`, joined
  with `+`: `$ Console + Clock`. A function that prints needs `$ Console`,
  and so does every function that calls it.
- A function that waits (input, files, timers) is named with `!`, and every
  call to it is written with `!`. Such a program's entry point is `main!`.
- Declarations are private unless `pub`. The entry point is `pub fn main`.

## References

Open the file the task needs. Each is short and covers one topic.

### How to, by scenario

Each page is one task as a complete program with its tests, then the
pieces it is made of.

| To | Read |
| --- | --- |
| build a command-line tool: options, `--help`, exit status | [cli-tool.md](references/how-to/cli-tool.md) |
| read standard input and transform it | [stdin-filter.md](references/how-to/stdin-filter.md) |
| parse a text or CSV file into typed records, with line-numbered errors | [text-records.md](references/how-to/text-records.md) |
| load configuration from JSON, environment variables and defaults | [configuration.md](references/how-to/configuration.md) |
| list, read, write and move files | [files-and-directories.md](references/how-to/files-and-directories.md) |
| call an HTTP API and decode JSON | [http-api.md](references/how-to/http-api.md) |
| run another program and read its output | [run-commands.md](references/how-to/run-commands.md) |
| do several waits at once, with timeouts and retries | [concurrent-work.md](references/how-to/concurrent-work.md) |
| share and update state: `&mut`, `box`, or a provider | [shared-state.md](references/how-to/shared-state.md) |
| model states, transitions and validation rules | [domain-model.md](references/how-to/domain-model.md) |
| start a project and split it into modules | [project-layout.md](references/how-to/project-layout.md) |
| translate a habit from Python, TypeScript, Rust or Go | [from-other-languages.md](references/how-to/from-other-languages.md) |

### Language

| File | Covers |
| --- | --- |
| [basics.md](references/language/basics.md) | scripts and `main`, comments, `:=` / `let` / `let mut`, numbers and conversions, strings |
| [collections.md](references/language/collections.md) | lists, maps, tuples, `for` and ranges, comprehensions, lazy iterators |
| [functions.md](references/language/functions.md) | functions, defaults and named arguments, closures, pipes, `if` / `match` / loops as expressions, `defer` |
| [data.md](references/language/data.md) | structs, methods, enums, `match` patterns, let-else, newtypes and aliases |
| [errors.md](references/language/errors.md) | `T?`, `Result`, `?`, `@error` enums, converting and reporting errors, exit codes |
| [traits.md](references/language/traits.md) | traits, generics and bounds, `dyn` trait values, `@derive`, operators, `Display` |
| [references.md](references/language/references.md) | values and copies, `&T` / `&mut T`, `box` and `*T`, stored closures, what may escape |
| [requirements.md](references/language/requirements.md) | rows after `$`, `$.use`, `$.with`, callbacks with rows, host capabilities |
| [suspension.md](references/language/suspension.md) | `!` functions and calls, `main!`, `join!`, `race!`, timeouts and retries |
| [modules.md](references/language/modules.md) | files as modules, `pub`, `use` forms and roots, doc comments |

### Standard library

| File | Covers |
| --- | --- |
| [imports.md](references/std/imports.md) | the prelude, and the import line for everything else |
| [text.md](references/std/text.md) | string methods, parsing, building text, characters, regex |
| [collections.md](references/std/collections.md) | list, map, iterator, optional and result methods; `Set`, `Deque`, `Heap` |
| [host.md](references/std/host.md) | console input, arguments, environment, files, time, JSON, random |

### Testing

| File | Covers |
| --- | --- |
| [unit-tests.md](references/testing/unit-tests.md) | `tests:` blocks, `it`, assertions, tables with `it_each`, properties with `it_prop` |
| [fakes.md](references/testing/fakes.md) | testing code that prints, reads files, reads the clock, or uses your own traits |
| [integration.md](references/testing/integration.md) | `tests/NAME.hd`, running the built program, snapshots, doc tests |

### The `hd` command

| File | Covers |
| --- | --- |
| [commands.md](references/cli/commands.md) | every command and flag an agent needs |
| [output.md](references/cli/output.md) | JSON diagnostics and fix-its, where output prints, exit statuses, panics |
| [packages.md](references/cli/packages.md) | `hd new`, `hd.toml`, `[capabilities]`, executables and tasks, dependencies, workspaces |

### Mistakes, by diagnostic code

Each row is what was written, the code `hd` reports, and what to write
instead.

| File | Codes such as |
| --- | --- |
| [syntax.md](references/mistakes/syntax.md) | `syntax-error`, `missing-let`, `trailing-block-position`, text that prints wrong |
| [types.md](references/mistakes/types.md) | `mixed-signedness`, `type-mismatch`, `non-reassignable-binding`, `unsatisfied-trait-bound`, `unknown-method`, `discarded-must-use-value` |
| [references.md](references/mistakes/references.md) | `not-writable`, `local-reference-escapes`, `conflicting-access`, `reference-equality` |
| [requirements.md](references/mistakes/requirements.md) | `missing-requirement`, `bang-call-outside-suspension`, `invalid-result-propagation`, `missing-capability`, `private-type-leak`, test and module errors |
| [std.md](references/mistakes/std.md) | a std name or method that does not exist, or needs an import |

Online: the [tour](https://herringtondarkholme.github.io/hd-lang/) runs each
topic in the browser.
