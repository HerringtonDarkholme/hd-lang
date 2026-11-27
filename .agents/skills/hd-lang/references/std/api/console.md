# `std.console`

Generated from `hd doc std.console`. Run `hd doc std.console.NAME` in a package for one item's full text.

## `Console`

```text
pub trait Console
```

The host capability to write unchanged output text and error text.

## `ConsoleInput`

```text
pub trait ConsoleInput
```

The host capability to read a line of input.

## `read_line`

```text
pub fn read_line!() -> Result[string?, ConsoleError] $ ConsoleInput
```

Returns the next line of input without its line ending, or `.None` at the end of input.

## `read_all`

```text
pub fn read_all!() -> Result[string, ConsoleError] $ ConsoleInput
```

Returns all the input that is left, as text, with `\r\n` read as `\n`.

## `MemoryConsole`

```text
pub struct MemoryConsole
```

The recording `Console` provider: it keeps exact text chunks for a test to read.

- `pub fn new() -> *mut MemoryConsole`: Returns an empty recording provider.
- `pub fn output(&self) -> List[string]`: The exact text chunks written through `write_sync`, in order.
- `pub fn error_output(&self) -> List[string]`: The exact text chunks written through `write_error_sync`, in order.

## `println`

```text
pub fn println[T < Display](value: T) -> void $ Console
```

Writes `value`'s `Display` text and a line ending to the `Console` provider that covers the call; it panics when the write fails.

## `print`

```text
pub fn print[T < Display](value: T) -> void $ Console
```

Writes Display text without a newline to the covering Console provider; it panics when the write fails.

## `eprintln`

```text
pub fn eprintln[T < Display](value: T) -> void $ Console
```

Like `println`, but it writes the line as error output.

## `ScriptedInput`

```text
pub struct ScriptedInput
```

The deterministic `ConsoleInput` provider: it answers from a list of lines, with no host access.

- `pub fn new(lines: List[string]) -> *mut ScriptedInput`: Returns the provider that answers `lines` in order, then the end of input.

## `ConsoleStream`

```text
pub enum ConsoleStream
```

The standard output, standard error, and standard input streams.

## `ConsoleError`

```text
pub enum ConsoleError
```

The console's heap-free errors: closed, not granted, or another failure code.
