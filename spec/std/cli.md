# Cli

Status: standard library specification draft.

This chapter defines `std.cli`, which `lib/std` writes in ordinary hd
over the language tier:

- `Cli`, which declares a program's flags, options, and positionals;
- `parse` and `parse_args`, which read arguments into `Parsed`;
- `CliError`, the parse errors;
- `usage`, the generated help text.

The arguments themselves come from `Args`
([Program Arguments](host.md#program-arguments)). What `hd` passes as a
program's arguments is CLI tier
([Program Arguments](../cli/command-line.md#program-arguments)).

## Declaring Arguments

A `Cli` value lists what a program accepts. Each builder method returns a
new `Cli` with one more declaration:

```text
use std.cli.Cli

fn tool() -> Cli:
    Cli::new("count").flag("verbose", 'v', "print each file").option("out", 'o', "write the total here").positional("pattern", "the files to count")
```

| Rule | Method | Declares |
| --- | --- | --- |
| r[std-cli.cli.new] `new` | `pub fn new(program: string) -> Cli` | nothing; `program` names the program in the usage text |
| r[std-cli.cli.flag] `flag` | `pub fn flag(self, name: string, short: char?, help: string) -> Cli` | a **flag**, an argument that takes no value |
| r[std-cli.cli.option] `option` | `pub fn option(self, name: string, short: char?, help: string) -> Cli` | an **option**, an argument that takes one value |
| r[std-cli.cli.positional] `positional` | `pub fn positional(self, name: string, help: string) -> Cli` | a required **positional**, an argument that is not a flag or an option |

1. r[std-cli.cli.decl] `std.cli` declares the data type `Cli`, with private fields, and the methods in this chapter. Code imports it, as in `use std.cli.Cli`.
2. r[std-cli.cli.builder] Each builder method returns a new `Cli` with every declaration of `self`, then the new one. `self` is unchanged.
3. r[std-cli.cli.names] A flag or an option has a long name, `name`, and may have a short name, `short`. `help` is its text in the usage.
4. r[std-cli.cli.duplicate] Declaring a long or short name of a flag or option a second time panics, and so does declaring a positional's name a second time. Panic: `explicit-panic`.
5. r[std-cli.cli.positional.order] Positionals are filled in declaration order, and each one is required.

> **Why.** A builder needs no fact types and no derivation, and it reads
> like Go's `flag` and Node's `util.parseArgs` tables. A typed form, which
> derives a parser from a data type as `Deserialize` does, can follow over the
> same parser.

## Parsing

`parse` reads a list of arguments, and `parse_args` reads the program's
own:

```text
use std.cli.{Cli, CliError}
use std.host.Args

fn tool() -> Cli:
    Cli::new("count").flag("verbose", 'v', "print each file").option("out", 'o', "write the total here").positional("pattern", "the files to count")

fn target() -> string $ Args:
    match tool().parse_args():
        .Ok(parsed) => parsed.value("out").unwrap_or("stdout")
        .Err(error) => "$error"
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-cli.parse.decl] `parse` | `pub fn parse(self, arguments: List[string]) -> Result[Parsed, CliError]` | what `arguments` hold, read from first to last against the declarations |
| r[std-cli.parse-args] `parse_args` | `pub fn parse_args(self) -> Result[Parsed, CliError] $ Args` | `parse` of the `list()` of the `Args` provider that covers the call |

1. r[std-cli.parse.plain] `parse` and `parse_args` are plain calls, not bang calls. `parse` has the empty requirement row.

### Argument Forms

Each argument is read by its form:

| Rule | Argument | Read as |
| --- | --- | --- |
| r[std-cli.form.long-flag] Long flag | `--name` | the flag `name` is set |
| r[std-cli.form.short-flag] Short flag | `-c` | the flag whose short name is `c` is set |
| r[std-cli.form.long-option] Long option | `--name VALUE` | the next argument is the value of the option `name` |
| r[std-cli.form.attached] Attached value | `--name=VALUE` | the text after the first `=` is the value of the option `name` |
| r[std-cli.form.short-option] Short option | `-c VALUE` | the next argument is the value of the option whose short name is `c` |
| r[std-cli.form.end] End of options | `--` | dropped; every later argument is a positional |
| r[std-cli.form.dash] Lone dash | `-` | a positional |
| r[std-cli.form.positional] Positional | an argument that does not start with `-` | a positional |

1. r[std-cli.form.interleaved] Flags, options, and positionals may come in any order before `--`.
2. r[std-cli.form.no-clusters] Only the forms in the table name a flag or an option. So `-vq`, `-ofile`, and `-o=file` are each one unknown option.
3. r[std-cli.form.value-dash] An option's value is the next argument even when that argument starts with `-` or is `--`.
4. r[std-cli.form.attached.text] An attached value may be empty, and may hold `=`: `--out=a=b` gives `a=b`.
5. r[std-cli.form.repeat-flag] A flag given more than once is set once.
6. r[std-cli.form.repeat-option] An option given more than once keeps its last value.

> **Why.** Clusters and attached short values make `-ofile` ambiguous
> beside a flag `-f`. Go's `flag` package leaves them out too, and a script
> rarely misses them.

### Parse Errors

`parse` returns one of four errors:

```text
pub enum CliError:
    UnknownOption(text: string)
    MissingValue(name: string)
    UnexpectedValue(name: string)
    MissingPositional(name: string)
```

| Rule | Variant | When | `Display` text |
| --- | --- | --- | --- |
| r[std-cli.error.unknown] Unknown option | `UnknownOption` | an argument that starts with `-`, is not `-` or `--`, and names no declared flag or option | `unknown option TEXT` |
| r[std-cli.error.missing-value] Missing value | `MissingValue` | an option without `=` is the last argument | `option --NAME needs a value` |
| r[std-cli.error.unexpected-value] Unexpected value | `UnexpectedValue` | a flag is written `--name=VALUE` | `flag --NAME takes no value` |
| r[std-cli.error.missing-positional] Missing positional | `MissingPositional` | fewer positionals are given than declared | `missing argument <NAME>` |

1. r[std-cli.error.decl] `std.cli` declares the enum `CliError` with the four variants above.
2. r[std-cli.error.traits] `CliError` implements `Eq`, `Debug`, and `Display`.
3. r[std-cli.error.text] `UnknownOption`'s `text` is the argument as written, up to its first `=` when it starts with `--`.
4. r[std-cli.error.name] The `name` of `MissingValue` and `UnexpectedValue` is the long name of the flag or option. The `name` of `MissingPositional` is the first positional not given.
5. r[std-cli.error.display] In the `Display` text, `TEXT` and `NAME` stand for the variant's field.
6. r[std-cli.error.first] `parse` returns the first error in argument order. A missing positional is checked after the last argument.
7. r[std-cli.error.help] When a flag named `help` is declared and set, `parse` does not check for a missing positional.

> **Why.** `tool --help` must reach the program even when `tool` requires
> a positional, as `argparse` and `clap` let it. `Cli` declares no
> `--help` itself, so the program prints `usage` when the flag is set.

### Parsed Arguments

`Parsed` holds what `parse` read:

```text
pub data Parsed:
    pub flags: List[string]
    pub values: Map[string, string]
    pub positionals: List[string]
```

1. r[std-cli.parsed.decl] `std.cli` declares the data type `Parsed` with the public fields above. It implements `Eq` and `Debug`.
2. r[std-cli.parsed.flags] `flags` holds the long name of every flag that was set, each once, in the order of first appearance.
3. r[std-cli.parsed.values] `values` maps the long name of every option that was given to its value.
4. r[std-cli.parsed.positionals] `positionals` holds every positional in argument order, including those past the declared ones.
5. r[std-cli.parsed.flag] `pub fn flag(self, name: string) -> bool` returns whether `flags` holds `name`.
6. r[std-cli.parsed.value] `pub fn value(self, name: string) -> string?` returns the value of `name` in `values`, or `.None`.
7. r[std-cli.parsed.undeclared] A name that nothing declares is never set, so `flag` returns `false` and `value` returns `.None` for it.

> **Note.** A declared positional is required, so `positionals[0]` holds
> the first one whenever `parse` succeeds.

## Usage Text

`usage` returns the help text. For the `Cli` above, with one more flag
`dry-run` that has no short name and no help, it is:

```console
usage: count [options] <pattern>

arguments:
  pattern            the files to count

options:
  -v, --verbose      print each file
  -o, --out <value>  write the total here
      --dry-run
```

1. r[std-cli.usage.decl] `pub fn usage(self) -> string` returns the lines below, joined by `\n`, with no final line break.
2. r[std-cli.usage.line] The first line is `usage: `, the program name, ` [options]` when a flag or an option is declared, and ` <name>` for each positional, in declaration order.
3. r[std-cli.usage.arguments] When a positional is declared, an empty line and the line `arguments:` follow, then one entry per positional, in declaration order. A positional's term is its name.
4. r[std-cli.usage.options] When a flag or an option is declared, an empty line and the line `options:` follow, then one entry per flag or option, in declaration order.
5. r[std-cli.usage.term] A flag's or option's term is `-c, --name`, or four spaces and `--name` when it has no short name. An option's term ends with ` <value>`.
6. r[std-cli.usage.entry] An entry is two spaces, its term padded with spaces to the width of the longest term in the text, two spaces, and its help.
7. r[std-cli.usage.width] A width counts characters, as `pad_end` does.
8. r[std-cli.usage.no-help] An entry whose help is empty is two spaces and its term, with no padding.
9. r[std-cli.usage.no-help-flag] Neither `usage` nor `parse` adds a flag that the `Cli` does not declare, `--help` included.

> **Why.** One column of help text is the layout of Go's `flag` output and
> of `hd help`. The four spaces keep the long names of every term aligned.

See also: [Program Arguments](host.md#program-arguments),
[Map Providers](host.md#map-providers),
[Splitting And Padding](text.md#splitting-and-padding).
