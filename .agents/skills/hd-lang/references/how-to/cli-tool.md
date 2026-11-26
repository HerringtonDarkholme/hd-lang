# How To: Build A Command-Line Tool

**Scenario.** `wordtop notes.txt -n 5 -i` prints the most frequent words
of a file. It needs options, a `--help`, an error on standard error, and
an exit status.

**Derive the parser from a struct.** `@derive(Cli)` turns a struct's
fields into arguments, its `##` doc comments into the help text, and the
parse into a typed value. Don't hand-build an `ArgParser` for a fixed set
of arguments.

## The program

```hd
use std.cli.{Args, Cli, ScriptedArgs, positional, short}
use std.collections.counts
use std.console.{MemoryConsole, eprintln}
use std.fs.{FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.path.Path
use std.process.ExitCode

## Print the most frequent words of a file
@derive(Cli)
struct Options:
    ## the file to read
    @positional
    file: Path
    ## how many words to print
    @short('n')
    top: usize = 3
    ## count Tea and tea together
    @short('i')
    ignore_case: bool

# The work is a plain function: no row, so it is easy to test.
fn top_words(text: string, options: &Options) -> List[(string, usize)]:
    source := if options.ignore_case: text.lower() else: text
    counts(source.split_whitespace()).most_common(options.top)

pub fn main!() -> ExitCode $ Args + Console + FsRead:
    options := match Options::parse_args():
        .Ok(options) => options
        .Err(error) => return error.exit()       # usage error: status 2; --help: status 0
    text := match read_text!(options.file):
        .Ok(text) => text
        .Err(error) =>
            eprintln("wordtop: ${options.file}: $error")
            return ExitCode(1)
    for (word, count) in top_words(text, &options):
        println("$count $word")
    ExitCode(0)

tests:
    it("prints the top words of a file"):
        fs := MemoryFs::new()
        console := MemoryConsole::new()
        $.with(FsWrite=fs):
            _ := write_text!(Path("notes.txt"), "tea Tea cake tea")
        arguments := box(ScriptedArgs::new("wordtop", ["notes.txt", "-n", "1", "-i"]))
        $.with(Args=arguments, FsRead=fs, Console=console):
            assert_equal(main!(), ExitCode(0))
        assert_equal(console.output().join(""), "3 tea\n")

    it("parses without a host"):
        options := Options::parse(["notes.txt", "--top", "5"]).expect("valid arguments")
        assert_equal(options.top, 5)
        assert(!options.ignore_case)

    it("reports a missing file on standard error"):
        console := MemoryConsole::new()
        arguments := box(ScriptedArgs::new("wordtop", ["nope.txt"]))
        $.with(Args=arguments, FsRead=MemoryFs::new(), Console=console):
            assert_equal(main!(), ExitCode(1))
        assert_equal(console.error_output().join(""), "wordtop: nope.txt: not found: nope.txt\n")
```

Run it with the arguments after `--`:

```sh
hd wordtop.hd -- notes.txt -n 5 -i
hd wordtop.hd -- --help
```

The generated help:

```text
usage: wordtop [options] <file>

Print the most frequent words of a file

arguments:
  file               the file to read

options:
  -n, --top <value>  how many words to print
  -i, --ignore-case  count Tea and tea together
  -h, --help         Print this help.
```

A bad value exits with status 2: `invalid value for --top: invalid digit
at position 0`.

## The pieces

- **Fields become arguments,** named in kebab-case (`ignore_case` is
  `--ignore-case`), in declaration order:
  - `bool` is a flag;
  - a field with a default (`top: usize = 3`) is optional;
  - `T?` reads `.None` when absent;
  - `List[T]` collects every occurrence;
  - any other field is required.
  Field types: `string`, `char`, the integers, `f32`/`f64`, `Path`,
  `Duration`, a newtype over one, or another `Cli` type.
- **Field facts:** `@short('n')` adds `-n`. `@positional` reads a plain
  argument instead of `--name`. A `List[T]` positional takes the rest and
  must come last.
- **Help text:** the struct's `##` comment is the description, and each
  field's `##` comment is its help line. `--help` is built in.
- **A closed set of values:** `@derive(Cli)` on a payload-free enum
  (`enum Format: Json, Markdown`) accepts `--format json` and lists the
  choices in the help.
- **Subcommands:** `@derive(Cli)` on an enum whose variants carry one
  `Cli` struct each (`enum Command: Build(BuildOptions), Stop`). A struct
  field of that type (`command: Command`) reads `build --out site`.
  Options before the command name are global.
- **Parse:**
  - `Options::parse_args()` reads the host's arguments and needs `$ Args`;
  - `Options::parse(["--top", "5"])` is pure, for tests.
  - Both return `Result[Options, CliError]`. `error.exit()` prints the
    usage or the help and picks the status.
- **Rules on values**, such as a range: add `Validate` to the derive and
  a rule fact to the field, `@range(1, 100)` from `std.validate`, then
  check `options.validated()` after parsing. Its error names the field:
  `$.top: must be within the allowed range`.
- **Errors** go to standard error with `eprintln`, and `main` returns a
  failing `ExitCode`.
- **In a package**, `hd.toml` must grant what `main` names: `Args = true`,
  `Console = true`, `FsRead = true` ([cli/packages.md](../cli/packages.md)).
- **When the arguments aren't known until run time,** use the builder
  underneath: `ArgParser::new(..).arg(Arg { .. })`, then `parse_args()`
  and read the `Parsed` (`value`, `flag`, `values`, `value_as::[T]`).
  `args()` from `std.cli` gives the raw list ([std/host.md](../std/host.md)).
