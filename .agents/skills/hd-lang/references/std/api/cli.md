# `std.cli`

Generated from `hd doc std.cli`. Run `hd doc std.cli.NAME` in a package for one item's full text.

## `ArgKind`

```text
pub enum ArgKind
```

## `Arg`

```text
pub struct Arg
```

One declaration. Options take a value, flags do not, and positionals follow declaration order.

## `ArgParser`

```text
pub struct ArgParser
```

A command line's declarations, changed in place through its mutable reference.

- `pub fn new(program: string, about: string = "") -> *mut ArgParser`
- `pub fn arg(*mut self, arg: Arg) -> *mut ArgParser`
- `pub fn command(*mut self, name: string, parser: ArgParser) -> *mut ArgParser`: Adds a command's parser by value; its options belong only to that command.
- `pub fn parse(&self, arguments: List[string]) -> Result[Parsed, CliError]`
- `pub fn parse_args(&self) -> Result[Parsed, CliError] $ Args`
- `pub fn usage(&self) -> string`

## `Parsed`

```text
pub struct Parsed
```

- `pub fn command(self) -> (string, Parsed)?`: The selected command and a copy of its parsed arguments, when present.
- `pub fn positionals(self) -> List[string]`: A copy of every positional in input order, including extra positionals.
- `pub fn flag(self, name: string) -> bool`: True when the flag `name` was present.
- `pub fn value(self, name: string) -> string?`: The value of the option `name`, in `.Some`, or `.None` when it was absent.
- `pub fn values(self, name: string) -> List[string]`: A copy of every value of the named option or positional, in argument order.
- `pub fn value_as[T < Parse[Error = E], E < Display](self, name: string) -> Result[T?, CliError]`: The value of the option `name` read as a `T`: `.None` when absent, an error naming the option when unreadable.

## `CliError`

```text
pub enum CliError
```

Why parsing failed: an unknown option, a missing value or positional, an unexpected value, or a value that does not read as its type.

- `pub fn exit_code(self) -> ExitCode`: Help exits successfully; a usage error exits with status 2.
- `pub fn exit(self) -> ExitCode $ Console`: Writes help to stdout or a usage error to stderr, then returns its status.

## `Cli`

```text
pub trait Cli
```

A type read from command-line declarations and parsed values.

## `Short`

```text
pub struct Short[F < Cli]
```

A field's short option spelling; its long name stays unchanged.

## `short`

```text
pub fn short[F < Cli](letter: char) -> Short[F]
```

Declares a short option for a CLI-readable field.

## `Positional`

```text
pub struct Positional[F < Cli]
```

Marks a CLI-readable field as positional.

## `positional`

```text
pub fn positional[F < Cli]() -> Positional[F]
```

Declares a positional field, in declaration order.

## `Args`

```text
pub trait Args
```

The host capability to read the process's arguments.

## `args`

```text
pub fn args() -> List[string] $ Args
```

The process's arguments after the program name.

## `ScriptedArgs`

```text
pub struct ScriptedArgs
```

The deterministic `Args` provider: a fixed list of arguments, with no host access.

- `pub fn new(program: string, values: List[string]) -> ScriptedArgs`: Returns the provider whose arguments are `program` then `values`.
