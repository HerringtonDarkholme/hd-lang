# Argument Parser

Status: standard library specification draft.

This chapter defines `std.cli`, which `lib/std` writes in ordinary hd
over the language tier:

- `ArgParser`, which declares flags, options, positionals, and subcommands;
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

fn tool() -> *mut ArgParser:
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
2. r[std-cli.parser.new-help] `ArgParser::new(program: string, about: string = "") -> *mut ArgParser` returns a new parser with automatic help.
3. r[std-cli.parser.arg] `arg(*mut self, arg: Arg) -> *mut ArgParser` appends a declaration in place and returns the same parser reference.
4. r[std-cli.parser.kind] `ArgKind.Flag` takes no value, `Option` takes one value, and `Positional` takes a positional value.
5. r[std-cli.parser.names] An argument's `name` is its lookup name; flags and options also accept `--name` and an optional short form.
6. r[std-cli.parser.duplicate] Declaring any argument name twice, any short option twice, or a short form on a positional panics. Panic: `explicit-panic`.
7. r[std-cli.parser.positional-order] Positionals fill declarations in order and are required only when their `required` field is true.
8. r[std-cli.parser.indexed] Declaration and parsing should take linear time in declaration and argument counts, using indexed option lookup and in-place appends.
9. r[std-cli.parser.choices] `choices` limits supplied values when nonempty; an empty list allows any value.
10. r[std-cli.parser.repeated-values] `repeated: true` permits repeated value-taking options and positional rest values; its default is false.
11. r[std-cli.parser.help] Every parser declares the value-free flag `help`, with short form `h` and description `Print this help.`.

> **Why.** A builder needs no fact types and no derivation, and it reads
> like Go's `flag` and Node's `util.parseArgs` tables. A typed form, which
> derives a parser from a struct as `Deserialize` does, can follow over the
> same parser.

## Subcommands

A subcommand has its own argument declarations, as in `ArgParser::new("notes").command("clean", *ArgParser::new("notes clean"))`.

1. r[std-cli.command.decl] `command(*mut self, name: string, parser: ArgParser) -> *mut ArgParser` adds a command and returns the parent parser reference.
2. r[std-cli.command.snapshot] `command` stores the supplied parser by value; later changes to the original builder do not change the stored command.
3. r[std-cli.command.no-positionals] Declaring a command beside a positional, in either order, panics. Panic: `explicit-panic`.
4. r[std-cli.command.duplicate] Declaring a command name twice panics. Panic: `explicit-panic`.
5. r[std-cli.command.lookup] The first plain argument names a command through one indexed lookup.
6. r[std-cli.command.scope] The child parses only arguments after its name, with its own declarations; global options are read before the command.
7. r[std-cli.command.unknown] An undeclared command name returns `CliError.UnknownCommand(text)` with the supplied name as `text`.
8. r[std-cli.command.missing-required] A parser with required commands returns `CliError.MissingCommand` when no command was selected.
9. r[std-cli.command.end-scope] An outer `--` makes its following command name plain, but does not disable option parsing within the selected child.
10. r[std-cli.command.cost] Delegation should pass the original input and its offset without copying the remaining arguments at each nesting level.

## Parsing

`parse` reads a list of arguments, and `parse_args` reads the program's
own:

```text
use std.cli.{Arg, ArgParser, CliError}
use std.host.Args

fn tool() -> *mut ArgParser:
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
| r[std-cli.form.long-user-flag] Long flag | `--name` | a non-help flag `name` is set |
| r[std-cli.form.short-user-flag] Short flag | `-c` | a non-help flag whose short name is `c` is set |
| r[std-cli.form.long-option] Long option | `--name VALUE` | the next argument is the value of the option `name` |
| r[std-cli.form.attached] Attached value | `--name=VALUE` | the text after the first `=` is the value of the option `name` |
| r[std-cli.form.short-option] Short option | `-c VALUE` | the next argument is the value of the option whose short name is `c` |
| r[std-cli.form.end-scope] End of options | `--` | dropped; later arguments in this parser are plain arguments |
| r[std-cli.form.dash-scope] Lone dash | `-` | a plain argument |
| r[std-cli.form.plain-argument] Positional | an argument that does not start with `-` | a plain argument |

1. r[std-cli.form.interleaved] Flags, options, and positionals may come in any order before `--`.
2. r[std-cli.form.no-clusters] Only the forms in the table name a flag or an option. So `-vq`, `-ofile`, and `-o=file` are each one unknown option.
3. r[std-cli.form.value-dash] An option's value is the next argument even when that argument starts with `-` or is `--`.
4. r[std-cli.form.attached.text] An attached value may be empty, and may hold `=`: `--out=a=b` gives `a=b`.
5. r[std-cli.form.repeat-user-flag] A non-help flag given more than once is set once.
6. r[std-cli.form.repeat-declared] An option with `repeated: true` retains every supplied value in argument order, and `value` returns the last.
7. r[std-cli.form.repeat-refused] A second occurrence of a value-taking option with `repeated: false` returns `InvalidValue`.
8. r[std-cli.form.plain-command] In a parser with commands, the first plain argument selects a command; otherwise, plain arguments are positionals.

```text
use std.cli.{Arg, ArgParser}

fn tool() -> *mut ArgParser:
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
    UnknownCommand(text: string)
    MissingCommand
    Help(usage: string)
```

| Rule | Variant | When | `Display` text |
| --- | --- | --- | --- |
| r[std-cli.error.unknown] Unknown option | `UnknownOption` | an argument that starts with `-`, is not `-` or `--`, and names no declared flag or option | `unknown option TEXT` |
| r[std-cli.error.missing-value] Missing value | `MissingValue` | an option without `=` is the last argument | `option --NAME needs a value` |
| r[std-cli.error.unexpected-value] Unexpected value | `UnexpectedValue` | a flag is written `--name=VALUE` | `flag --NAME takes no value` |
| r[std-cli.error.missing-argument] Missing argument | `MissingArgument` | a required flag, option or positional is absent | `missing argument NAME` |

1. r[std-cli.error.enum-help] `CliError` declares the eight variants above, all of which `parse` may return.
2. r[std-cli.error.required-argument] `MissingArgument` names a missing required argument as `--name` for a flag or option, or `<name>` for a positional.
3. r[std-cli.error.traits] `CliError` implements `Eq`, `Debug`, and `Display`.
4. r[std-cli.error.text] `UnknownOption`'s `text` is the argument as written, up to its first `=` when it starts with `--`.
5. r[std-cli.error.option-name] `MissingValue` and `UnexpectedValue` hold the option or flag’s long name.
6. r[std-cli.error.display] In the `Display` text, `TEXT`, `NAME`, and `REASON` stand for the variant's field.
7. r[std-cli.error.first-required] `parse` returns the first error in argument order, then checks required arguments in declaration order.
8. r[std-cli.error.help-stop] `-h` or `--help` immediately returns `CliError.Help(usage)` for the current command, before reading later arguments or checking required arguments.
9. r[std-cli.parse.choices] A value outside a nonempty `choices` list returns `InvalidValue(name, reason)`, with `reason` equal to `expected CHOICES, got VALUE`.
10. r[std-cli.parse.choices-text] `CHOICES` joins the declared choices with ` or `, and `VALUE` is the supplied value.
11. r[std-cli.parse.repetition-text] A refused repetition returns `InvalidValue(name, "argument cannot be repeated")`.
12. r[std-cli.command.error-unknown-text] `UnknownCommand(text)` displays `unknown command TEXT`, with the supplied command name for `TEXT`.
13. r[std-cli.command.error-missing-text] `MissingCommand` displays `missing command`.

14. r[std-cli.error.help-display] The `Display` text of `Help(usage)` is exactly `usage`.
15. r[std-cli.error.help-value] Help is recognized only as an option: an option value or a word after `--` retains its ordinary meaning.

> **Why.** Help succeeds even when an otherwise required command or argument
> is absent, as argparse and clap allow.

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

1. r[std-cli.error.exit-code-help] `CliError` has the method `pub fn exit_code(self) -> ExitCode`. It returns `ExitCode(0)` for `Help`, otherwise `ExitCode(2)`.
2. r[std-cli.error.exit-help] `CliError` has the method `pub fn exit(self) -> ExitCode $ Console`. It writes `Help` to standard output or other errors to standard error, then returns `exit_code()`.

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
8. r[std-cli.parsed.command] `command(self) -> (string, Parsed)?` returns the selected command name and copied child result, or `.None` without a command.

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

## Typed Records

The **`Cli` trait** reads typed command-line values over `ArgParser`.

```text
use std.cli.Cli

@derive(Cli)
struct Pack:
    verbose: bool
    level: u8 = 6
    exclude: List[string]
    limit: u32?

fn compression() -> u8:
    Pack::parse(["--level", "9"]).expect("valid arguments").level
```

1. r[std-cli.typed.trait] `Cli` declares the methods below.

| Method | Signature |
| --- | --- |
| Declaration | `fn declare(parser: *mut ArgParser, arg: Arg) -> void` |
| Reading | `fn read(parsed: Parsed, arg: Arg) -> Result[Self, CliError]` |
| Parser | `fn parser() -> *mut ArgParser` |
| Pure parsing | `fn parse(arguments: List[string]) -> Result[Self, CliError]` |
| Host parsing | `fn parse_args() -> Result[Self, CliError] $ Args` |

2. r[std-cli.typed.parser] The default `parser` creates a parser and calls `Self::declare` with an empty name and `required: true`.
3. r[std-cli.typed.parse] The default `parse` parses its arguments with `Self::parser()`, then calls `Self::read` with the root declaration.
4. r[std-cli.typed.parse-args] The default `parse_args` uses the covering `Args` provider's program name and argument list.
5. r[std-cli.typed.requirements] `parse` is pure, and `parse_args` requires only `Args`.
6. r[std-cli.typed.derive] `@derive(Cli)` on a record declares its fields through `Describer` and builds the result through `Source`.
7. r[std-cli.typed.fields] Record fields become arguments with kebab-case names in declaration order.
8. r[std-cli.typed.defaults] An absent field with a declared default uses that default.
9. r[std-cli.typed.required] An absent required scalar returns `MissingArgument("--name")`, using its kebab-case name.
10. r[std-cli.typed.bool] A named `bool` field declares a flag and reads false when absent, unless its field default applies.
11. r[std-cli.typed.scalars] The scalar types below implement `Cli` through their `Parse` implementations.

| Types |
| --- |
| `string`, `char` |
| `i8`, `i16`, `i32`, `i64` |
| `u8`, `u16`, `u32`, `u64`, `usize` |
| `f32`, `f64` |
| `Path`, `Duration` |

12. r[std-cli.typed.scalar-errors] A refused scalar returns `InvalidValue(name, reason)`, with its argument name and the parse error's `Display` text.
13. r[std-cli.typed.containers] `Option[T]` and `List[T]` implement `Cli` when `T` implements `Cli`.
14. r[std-cli.typed.optional] An absent optional field reads `.None`, unless its field default applies.
15. r[std-cli.typed.list-values] For value-taking or flag declarations, a list reads every occurrence in order, or `[]` when absent.
16. r[std-cli.typed.flag-list] A `List[bool]` flag field reads one `true` per occurrence.
17. r[std-cli.typed.view] Each list element is read through a constant-time one-value `Parsed` view over shared storage.
18. r[std-cli.typed.bounds] Every field of a derived record must implement `Cli`, checked at the derive.
19. r[std-cli.typed.newtype] A newtype derives `Cli` through its base type.
20. r[std-cli.typed.nested-record] Parsing a record-typed field returns `InvalidValue` naming the field and explaining that subcommands are an enum.

> **Why.** The typed layer reuses the builder's argument grammar and errors.
> Records do not implicitly flatten or become subcommands.

### Enum Value Sets

A payload-free enum derives a closed set of CLI values.

```text
use std.cli.Cli

@derive(Cli)
enum Format:
    Json
    Markdown

@derive(Cli)
struct Report:
    format: Format

fn report() -> Report:
    Report::parse(["--format", "json"]).expect("valid format")
```

1. r[std-cli.enum-values.derive] `@derive(Cli)` on a payload-free enum reads the selected variant from one argument value.
2. r[std-cli.enum-values.names] Accepted values are the kebab-case variant names, matched exactly.
3. r[std-cli.enum-values.invalid] An unknown value returns `InvalidValue(name, reason)`, with `reason` equal to `expected CHOICES, got VALUE`.
4. r[std-cli.enum-values.choice-order] `CHOICES` joins the kebab-case names in declaration order with ` or `.
5. r[std-cli.enum-values.help] The generated argument's `choices` contains those names, so ordinary usage text lists them.
6. r[std-cli.enum-values.composition] Enum values compose with record defaults, `Option`, `List`, and newtypes derived through the enum.

> **Why.** The value set uses the builder's existing choice validation and
> help layout, while `Source` returns the actual enum variant.

### Short Field Facts

A `Short` field fact adds one short spelling to a typed CLI argument.

```text
use std.cli.{Cli, short}

@derive(Cli)
struct Pack:
    @short('v')
    verbose: bool

fn verbose() -> bool:
    Pack::parse(["-v"]).expect("valid flag").verbose
```

1. r[std-cli.short.fact] `Short[F < Cli]` is a typed field fact with the public field `letter: char`.
2. r[std-cli.short.constructor] `short[F < Cli](letter: char) -> Short[F]` constructs that fact.
3. r[std-cli.short.spelling] A derived field's `Short` fact supplies `Arg.short`, while its long kebab-case name remains unchanged.
4. r[std-cli.short.bound] A `Short` fact requires its field type to implement `Cli`.
5. r[std-cli.short.duplicates] Short spellings retain the builder's duplicate-name checks. Panic: `explicit-panic`.

> **Why.** The fact configures the existing builder argument, so both
> spellings share its typed value, presence, repetition policy, and errors.

### Positional Field Facts

A `Positional` fact reads a typed field from plain arguments.

```text
use std.cli.{Cli, positional}
use std.path.Path

@derive(Cli)
struct Files:
    @positional
    input: Path
    @positional
    extras: List[string]

fn files() -> Files:
    Files::parse(["notes.txt", "other.txt"]).expect("valid paths")
```

1. r[std-cli.positional.fact] `Positional[F < Cli]` is a typed field fact with no fields.
2. r[std-cli.positional.constructor] `positional[F < Cli]() -> Positional[F]` constructs that fact.
3. r[std-cli.positional.bound] A `Positional` fact requires its field type to implement `Cli`.
4. r[std-cli.positional.order] Marked fields declare positional arguments in field declaration order.
5. r[std-cli.positional.defaults] Positional fields retain their type's parsing, declared defaults, optional absence, and empty-list behavior.
6. r[std-cli.positional.required] An absent required positional returns `MissingArgument("<name>")`, using its kebab-case field name.
7. r[std-cli.positional.rest] A repeated positional, including a typed `List[T]` field, consumes the remaining plain arguments in order.
8. r[std-cli.positional.rest-values] `values(name)` returns every value of a repeated positional, and `value(name)` returns its last value.
9. r[std-cli.positional.rest-last] Declaring a positional after a repeated positional panics with a message naming both fields. Panic: `explicit-panic`.
10. r[std-cli.positional.value-short-conflict] A positional value argument retains the builder's refusal of short spellings. Panic: `explicit-panic`.

> **Why.** A rest list must be last among positional declarations, so later
> fields cannot become unreachable. Option declarations may follow it.

### Typed Subcommands

An enum with payload variants derives a set of subcommands.

```text
use std.cli.Cli

@derive(Cli)
struct BuildOptions:
    out: string = "site"

@derive(Cli)
enum Command:
    Build(BuildOptions)
    Stop

@derive(Cli)
struct Notes:
    command: Command

fn notes() -> Notes:
    Notes::parse(["build", "--out", "public"]).expect("valid command")
```

1. r[std-cli.typed-commands.derive] When any variant carries a payload, the enum's variants declare subcommands rather than option choices.
2. r[std-cli.typed-commands.names] Subcommand names are the kebab-case variant names.
3. r[std-cli.typed-commands.shape] Each variant must be unit or carry exactly one unnamed `Cli` payload.
4. r[std-cli.typed-commands.payload-bound] Each payload type must implement `Cli`, checked at the derive.
5. r[std-cli.typed-commands.payload] A selected command delegates declaration and reading to its payload's root CLI parser.
6. r[std-cli.typed-commands.unit] A selected unit command constructs its unit variant without reading a payload.
7. r[std-cli.typed-commands.scope] Global options precede the command name; arguments after it belong to the selected child's parser.
8. r[std-cli.typed-commands.required] A command field without a default is required unless its type is optional.
9. r[std-cli.typed-commands.optional] An absent `Command?` reads `.None`.
10. r[std-cli.typed-commands.default] An absent defaulted command field reads its default; a selected command overrides that default.
11. r[std-cli.typed-commands.nested] A command's payload may declare another command at the next level.
12. r[std-cli.typed-commands.shape-error] An unsupported payload shape returns `InvalidValue` when parsing, with a reason naming the declared variant.
13. r[std-cli.typed-commands.one-field] Multiple command fields at one level return `InvalidValue` naming both fields when parsing.
14. r[std-cli.typed-commands.no-repeated] A repeated command field, including one inside a wrapper, returns `InvalidValue` naming that field when parsing.
15. r[std-cli.typed-commands.help-scope] Child help uses the full command path, beginning with the `Args` program name for `parse_args`.
16. r[std-cli.typed-commands.optional-usage] An optional or defaulted command's usage term is `[<command>]`.
17. r[std-cli.typed-commands.no-field-facts] A command field carrying `Short` or `Positional` returns `InvalidValue` naming the field when parsing.

> **Why.** Each level selects one command and delegates its remaining
> input, without implicitly flattening records. Repeated commands would
> need a delimiter separating their child arguments.

### Documentation In Typed Help

Typed CLI help uses the declaration documentation carried by `std.structure`.

```text
use std.cli.Cli

## Pack archives for distribution.
@derive(Cli)
struct Pack:
    ## Print detailed progress.
    verbose: bool

fn pack_help() -> string:
    match Pack::parse(["--help"]):
        .Err(.Help(text)) => text
        _ => panic("expected help")
```

1. r[std-cli.typed-help.type-doc] A root derived CLI uses its type's `VariantInfo.type_doc` as the parser's about text.
2. r[std-cli.typed-help.field-doc] A derived record argument uses its `Member.doc` as argument help.
3. r[std-cli.typed-help.variant-doc] Unit-enum choice descriptions and subcommand descriptions use their variants' `VariantInfo.doc`.
4. r[std-cli.typed-help.choice-lines] Documented choices add declaration-ordered `name: description` lines to argument help, after any field documentation.
5. r[std-cli.typed-help.payload-doc] A subcommand's help includes its variant documentation followed by its payload type documentation, when both exist and differ.
6. r[std-cli.typed-help.current-command] `-h` and `--help` return `CliError.Help` for the current command, including only that scope's about text and argument declarations.
7. r[std-cli.typed-help.absent-doc] Absent documentation adds no description.

> **Why.** Help reads the same declaration metadata as every other derivation,
> and preserves the command parser's existing scope boundaries.

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
  -h, --help         Print this help.
```

1. r[std-cli.usage.parser] `pub fn usage(&self) -> string` returns the lines below, joined by `\n`, with no final line break.
2. r[std-cli.usage.command-terms] The usage line names the program, `[options]`, and any command or positional terms.
3. r[std-cli.usage.positional-term] A positional is `<name>` when required, and `[<name>]` otherwise.
4. r[std-cli.usage.arguments] When a positional is declared, an empty line and the line `arguments:` follow, then one entry per positional, in declaration order. A positional's term is its name.
5. r[std-cli.usage.options-help-last] An empty line and `options:` precede user-declared flags and options in declaration order, followed by automatic help.
6. r[std-cli.usage.term] A flag's or option's term is `-c, --name`, or four spaces and `--name` when it has no short name. An option's term ends with ` <value>`.
7. r[std-cli.usage.entry-choices] An entry starts with two spaces and its term, padded to the longest term only when help or choices follow.
8. r[std-cli.usage.width] A width counts characters, as `pad_end` does.
9. r[std-cli.usage.no-description] An entry with empty help and choices is two spaces and its term, without padding.
10. r[std-cli.usage.automatic-help] Automatic help is listed last as `-h, --help` with description `Print this help.`.

11. r[std-cli.usage.about] When `about` is nonempty, one empty line and its text follow the usage line.
12. r[std-cli.usage.builder] `usage` should write its result with one `StringBuilder`, without repeated concatenation of the accumulated text.
13. r[std-cli.usage.choices] Choices append `[choices: A, B]` after help, separated by one space when help is nonempty.
14. r[std-cli.usage.description] After a padded term, two spaces precede its help or choices.
15. r[std-cli.usage.commands] Declared commands appear after the program description and before argument or option sections, following an empty line and `commands:`.
16. r[std-cli.usage.command-order] Each command entry is two spaces and its name, in declaration order.
17. r[std-cli.usage.command-about] A nonempty command description follows its name, padded to the longest command name, and two spaces.


> **Why.** One column of help text is the layout of Go's `flag` output and
> of `hd help`. The four spaces keep the long names of every term aligned.

See also: [Program Arguments](host.md#program-arguments),
[Map Providers](host.md#map-providers),
[Splitting And Padding](text.md#splitting-and-padding).
