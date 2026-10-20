# Argument Parser

Status: standard library specification draft.

This chapter defines `std.cli`, which `lib/std` writes in ordinary hd
over the language tier:

- `ArgParser`, which declares a program's flags, options, and positionals;
- `parse` and `parse_args`, which read arguments into `Parsed`;
- `CliError`, the parse errors, and the exit status they give;
- `usage`, the generated help text.

The arguments themselves come from `Args`
([Program Arguments](host.md#program-arguments)). What `hd` passes as a
program's arguments is CLI tier
([Program Arguments](../cli/command-line.md#program-arguments)).

## Declaring Arguments

A `ArgParser` value lists what a program accepts. Its builder changes one shared parser through a mutable reference:

```text
use std.cli.{Arg, ArgParser}

fn tool() -> &mut ArgParser:
    ArgParser::new("count").arg(Arg { name: "verbose", kind: .Flag, short: 'v', help: "print each file" }).arg(Arg { name: "out", kind: .Option, short: 'o', help: "write the total here" }).arg(Arg { name: "pattern", kind: .Positional, required: true, help: "the files to count" })
```

```text
pub enum ArgKind:
    Flag
    Option
    Positional

pub struct Arg:
    pub name: string
    pub kind: ArgKind = .Option
    pub short: char? = .None
    pub help: string = ""
    pub required: bool = false
    pub choices: List[string] = []
    pub repeated: bool = false
```

1. r[std-cli.parser.decl-choices] `std.cli` declares `ArgParser`, a struct with private fields, and public `ArgKind` and `Arg` as shown above.
2. r[std-cli.parser.new] `ArgParser::new(program: string, about: string = "") -> &mut ArgParser` returns a new empty parser.
3. r[std-cli.parser.arg] `arg(&mut self, arg: Arg) -> &mut ArgParser` appends a declaration in place and returns the same parser reference.
4. r[std-cli.parser.kind] `ArgKind.Flag` takes no value, `Option` takes one value, and `Positional` takes a positional value.
5. r[std-cli.parser.names] An argument's `name` is its lookup name; flags and options also accept `--name` and an optional short form.
6. r[std-cli.parser.duplicate] Declaring any argument name twice, any short option twice, or a short form on a positional panics. Panic: `explicit-panic`.
7. r[std-cli.parser.positional-order] Positionals fill declarations in order and are required only when their `required` field is true.
8. r[std-cli.parser.indexed] Declaration and parsing should take linear time in declaration and argument counts, using indexed option lookup and in-place appends.
9. r[std-cli.parser.choices] `choices` limits supplied values when nonempty; an empty list allows any value.
10. r[std-cli.parser.repeated] `repeated` permits multiple occurrences of a value-taking option when true; its default is false.

> **Why.** A builder needs no fact types and no derivation, and it reads
> like Go's `flag` and Node's `util.parseArgs` tables. A typed form, which
> derives a parser from a struct as `Deserialize` does, can follow over the
> same parser.

## Parsing

`parse` reads a list of arguments, and `parse_args` reads the program's
own:

```text
use std.cli.{Arg, ArgParser, CliError}
use std.host.Args

fn tool() -> &mut ArgParser:
    ArgParser::new("count").arg(Arg { name: "verbose", kind: .Flag, short: 'v', help: "print each file" }).arg(Arg { name: "out", kind: .Option, short: 'o', help: "write the total here" }).arg(Arg { name: "pattern", kind: .Positional, required: true, help: "the files to count" })

fn target() -> string $ Args:
    match tool().parse_args():
        .Ok(parsed) => parsed.value("out").unwrap_or("stdout")
        .Err(error) => "$error"
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-cli.parse.parser] `parse` | `pub fn parse(&self, arguments: List[string]) -> Result[Parsed, CliError]` | what `arguments` hold, read from first to last against the declarations |
| r[std-cli.parse-args.parser] `parse_args` | `pub fn parse_args(&self) -> Result[Parsed, CliError] $ Args` | `parse` of the `list()` of the `Args` provider that covers the call |

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
6. r[std-cli.form.repeat-declared] An option with `repeated: true` retains every supplied value in argument order, and `value` returns the last.
7. r[std-cli.form.repeat-refused] A second occurrence of a value-taking option with `repeated: false` returns `InvalidValue`.

```text
use std.cli.{Arg, ArgParser}

fn tool() -> &mut ArgParser:
    ArgParser::new("count").arg(Arg { name: "verbose", kind: .Flag, short: 'v', help: "print each file" }).arg(Arg { name: "out", kind: .Option, short: 'o', help: "write the total here" }).arg(Arg { name: "pattern", kind: .Positional, required: true, help: "the files to count" })

fn out_of(arguments: List[string]) -> string:
    match tool().parse(arguments):
        .Ok(parsed) => parsed.value("out").unwrap_or("stdout")
        .Err(error) => "$error"

fn demo() -> List[string]:
    [out_of(["-o", "a.txt", "*.txt"]),   # a short option takes the next argument
     out_of(["--out=b.txt", "*.txt"]),   # an attached value
     out_of(["--", "--out"])]            # after --, a positional: "stdout"
```

> **Why.** Clusters and attached short values make `-ofile` ambiguous
> beside a flag `-f`. Go's `flag` package leaves them out too, and a script
> rarely misses them.

### Parse Errors

`CliError` reports declaration-relative parse failures and refused typed values
([Typed Values](#typed-values)):

```text
pub enum CliError:
    UnknownOption(text: string)
    MissingValue(name: string)
    UnexpectedValue(name: string)
    MissingArgument(name: string)
    InvalidValue(name: string, reason: string)
```

| Rule | Variant | When | `Display` text |
| --- | --- | --- | --- |
| r[std-cli.error.unknown] Unknown option | `UnknownOption` | an argument that starts with `-`, is not `-` or `--`, and names no declared flag or option | `unknown option TEXT` |
| r[std-cli.error.missing-value] Missing value | `MissingValue` | an option without `=` is the last argument | `option --NAME needs a value` |
| r[std-cli.error.unexpected-value] Unexpected value | `UnexpectedValue` | a flag is written `--name=VALUE` | `flag --NAME takes no value` |
| r[std-cli.error.missing-argument] Missing argument | `MissingArgument` | a required flag, option or positional is absent | `missing argument NAME` |

1. r[std-cli.error.enum-values] `CliError` declares the five variants above, all of which `parse` may return.
2. r[std-cli.error.required-argument] `MissingArgument` names a missing required argument as `--name` for a flag or option, or `<name>` for a positional.
3. r[std-cli.error.traits] `CliError` implements `Eq`, `Debug`, and `Display`.
4. r[std-cli.error.text] `UnknownOption`'s `text` is the argument as written, up to its first `=` when it starts with `--`.
5. r[std-cli.error.option-name] `MissingValue` and `UnexpectedValue` hold the option or flag’s long name.
6. r[std-cli.error.display] In the `Display` text, `TEXT`, `NAME`, and `REASON` stand for the variant's field.
7. r[std-cli.error.first-required] `parse` returns the first error in argument order, then checks required arguments in declaration order.
8. r[std-cli.error.help-required] A declared flag named `help`, when set, skips missing-required-argument checks.
9. r[std-cli.parse.choices] A value outside a nonempty `choices` list returns `InvalidValue(name, reason)`, with `reason` equal to `expected CHOICES, got VALUE`.
10. r[std-cli.parse.choices-text] `CHOICES` joins the declared choices with ` or `, and `VALUE` is the supplied value.
11. r[std-cli.parse.repetition-text] A refused repetition returns `InvalidValue(name, "argument cannot be repeated")`.

> **Why.** `tool --help` must reach the program even when `tool` requires
> a positional, as `argparse` and `clap` let it. `ArgParser` declares no
> `--help` itself, so the program prints `usage` when the flag is set.

### Exit Status

A command-line failure picks its exit status through `CliError`, not
through `Error`. `main` reports a parse error and exits with its code:

```text
use std.cli.{Arg, ArgParser}
use std.host.Args
use std.process.ExitCode

pub fn main() -> ExitCode $ Args + Console:
    cli := ArgParser::new("tool").arg(Arg { name: "level", help: "how loud" })
    parsed := match cli.parse_args():
        .Ok(parsed) => parsed
        .Err(error) => return error.exit()
    println(parsed.value("level").unwrap_or("quiet"))
    ExitCode(0)
```

With `--levl 3`, this `main` writes `unknown option --levl` as error
output and exits with status 2.

1. r[std-cli.error.exit-code] `CliError` has the method `pub fn exit_code(self) -> ExitCode`. It returns `ExitCode(2)` for every variant.
2. r[std-cli.error.exit] `CliError` has the method `pub fn exit(self) -> ExitCode $ Console`. It writes the error's `Display` text as `eprintln` does, then returns `exit_code()`.

> **Why.** As in clap, the exit code of a usage error belongs to the
> command-line error type. `Error` has no exit code, so `main` chooses one
> by calling `exit`.

### Parsed Arguments

`Parsed` holds what `parse` read:

```text
fn first(arguments: List[string]) -> string?:
    match tool().parse(arguments):
        .Ok(parsed) => parsed.value("pattern")
        .Err(_) => .None
```

1. r[std-cli.parsed.private] `Parsed` is a value with private storage and implements `Eq` and `Debug`; its methods expose copied values.
2. r[std-cli.parsed.flag-presence] `flag(self, name: string) -> bool` returns whether the declared flag was present.
3. r[std-cli.parsed.named-value] `value(self, name: string) -> string?` returns the last option value or the declared positional value, otherwise `.None`.
4. r[std-cli.parsed.positional-copy] `positionals(self) -> List[string]` returns a copy of every positional in input order, including undeclared extras.
5. r[std-cli.parsed.unknown] For an undeclared name, `flag` returns false and `value` returns `.None`.
6. r[std-cli.parsed.all-values] `values(self, name: string) -> List[string]` returns copied option or declared positional values for `name`, in argument order.
7. r[std-cli.parsed.values-empty] For flags, absent arguments, and undeclared names, `values` returns an empty list.

### Typed Values

`value_as` reads an option's value as any type that implements
[`Parse`](text.md#parsing-text):

```text
use std.cli.{Arg, ArgParser, CliError}

fn max_lines(args: List[string]) -> Result[usize, CliError]:
    parsed := ArgParser::new("tail").arg(Arg { name: "max", kind: .Option, short: .None, help: "lines to show" }).parse(args)?
    .Ok(parsed.value_as::[usize]("max")?.unwrap_or(10))
```

1. r[std-cli.parsed.value-as] `Parsed` has the method `pub fn value_as[T < Parse[Error = E], E < Display](self, name: string) -> Result[T?, CliError]`. The call names the type, as in `parsed.value_as::[usize]("max")`.
2. r[std-cli.value-as.absent] When `value(name)` is `.None`, `value_as` returns `.Ok(.None)`.
3. r[std-cli.value-as.present] When `value(name)` is `.Some(text)` and `T::parse(text)` returns `.Ok(value)`, `value_as` returns `.Ok(.Some(value))`.
4. r[std-cli.value-as.invalid] When `T::parse(text)` returns `.Err(error)`, `value_as` returns `.Err(CliError.InvalidValue(name, reason))`. `reason` is the `Display` text of `error`.
5. r[std-cli.value-as.display] The `Display` text of `InvalidValue` is `invalid value for --NAME: REASON`.

| `--max` value | `value_as::[usize]("max")` |
| --- | --- |
| not given | `.Ok(.None)` |
| `"25"` | `.Ok(.Some(25))` |
| `"x1"` | `.Err(CliError.InvalidValue("max", "invalid digit at position 0"))` |

> **Why.** The error names the option, so a program can print it as it
> is, and carries the text, not the parse error, so `CliError` stays one
> type for every `T`. An absent option is `.None`, the same as `value`,
> so `unwrap_or` supplies a default.

## Usage Text

`usage` returns the help text. For the `ArgParser` above, with one more flag
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

1. r[std-cli.usage.parser] `pub fn usage(&self) -> string` returns the lines below, joined by `\n`, with no final line break.
2. r[std-cli.usage.parser-line] The usage line names the program, optional options, and each positional as `<name>` when required or `[<name>]` otherwise.
3. r[std-cli.usage.arguments] When a positional is declared, an empty line and the line `arguments:` follow, then one entry per positional, in declaration order. A positional's term is its name.
4. r[std-cli.usage.options] When a flag or an option is declared, an empty line and the line `options:` follow. Then one entry per flag or option follows, in declaration order.
5. r[std-cli.usage.term] A flag's or option's term is `-c, --name`, or four spaces and `--name` when it has no short name. An option's term ends with ` <value>`.
6. r[std-cli.usage.entry-choices] An entry starts with two spaces and its term, padded to the longest term only when help or choices follow.
7. r[std-cli.usage.width] A width counts characters, as `pad_end` does.
8. r[std-cli.usage.no-description] An entry with empty help and choices is two spaces and its term, without padding.
9. r[std-cli.usage.parser-no-help] Neither `usage` nor `parse` adds a flag that the `ArgParser` does not declare, `--help` included.

10. r[std-cli.usage.about] When `about` is nonempty, one empty line and its text follow the usage line.
11. r[std-cli.usage.builder] `usage` should write its result with one `StringBuilder`, without repeated concatenation of the accumulated text.
12. r[std-cli.usage.choices] Choices append `[choices: A, B]` after help, separated by one space when help is nonempty.
13. r[std-cli.usage.description] After a padded term, two spaces precede its help or choices.

> **Why.** One column of help text is the layout of Go's `flag` output and
> of `hd help`. The four spaces keep the long names of every term aligned.

See also: [Program Arguments](host.md#program-arguments),
[Map Providers](host.md#map-providers),
[Splitting And Padding](text.md#splitting-and-padding).
