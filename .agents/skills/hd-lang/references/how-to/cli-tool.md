# How To: Build A Command-Line Tool

**Scenario.** `wordtop notes.txt -n 5 -i` prints the most frequent words
of a file. It needs options, a `--help`, an error on standard error, and
an exit status.

## The program

```hd
use std.cli.{Arg, ArgParser, CliError, Parsed}
use std.console.{MemoryConsole, eprintln}
use std.fs.{FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.host.{Args, ScriptedArgs}
use std.path.Path
use std.process.ExitCode

struct Options:
    path: string
    top: usize
    ignore_case: bool

fn parser() -> *mut ArgParser:
    ArgParser::new("wordtop", about="Print the most frequent words of a file")
        .arg(Arg { name: "file", kind: .Positional, required: true, help: "the file to read" })
        .arg(Arg { name: "top", short: 'n', help: "how many words to print" })
        .arg(Arg { name: "ignore-case", kind: .Flag, short: 'i', help: "count Tea and tea together" })

fn options(parsed: Parsed) -> Result[Options, CliError]:
    top := parsed.value_as::[usize]("top")?.unwrap_or(3)   # write the value type
    .Ok(Options {
        path: parsed.value("file").unwrap_or(""),
        top: top,
        ignore_case: parsed.flag("ignore-case"),
    })

# The work is a plain function: no row, so it is easy to test.
fn top_words(text: string, options: Options) -> List[(string, usize)]:
    source := if options.ignore_case: text.lower() else: text
    let mut counts: Map[string, usize] = {}
    for word in source.split_whitespace():
        *counts.get_or_insert(word, 0) += 1
    counts.most_common(options.top)

pub fn main!() -> ExitCode $ Args + Console + FsRead:
    parsed := match parser().parse_args():
        .Ok(parsed) => parsed
        .Err(error) => return error.exit()       # usage error: status 2; --help: status 0
    chosen := match options(parsed):
        .Ok(chosen) => chosen
        .Err(error) => return error.exit()
    text := match read_text!(Path(chosen.path)):
        .Ok(text) => text
        .Err(error) =>
            eprintln("wordtop: ${chosen.path}: $error")
            return ExitCode(1)
    for (word, count) in top_words(text, chosen):
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

## The pieces

- **Declare arguments** with `Arg`. `kind` is `.Option` (takes a value, the
  default), `.Flag` (present or not) or `.Positional`. Other fields:
  `short`, `help`, `required`, `choices`, `repeated`.
- **Read them** from `Parsed`: `value(name)` gives `string?`, `flag(name)`
  a `bool`, `values(name)` a list for a repeated option, and
  `value_as::[T](name)` a parsed value. Always write the type argument,
  as `value_as::[usize]("top")`.
- **Exit status.** Return `ExitCode` from `main`. `error.exit()` on a
  `CliError` prints the usage and picks the status. Without `std.cli`,
  `args()` from `std.host` gives the raw list
  ([std/host.md](../std/host.md)).
- **Errors** go to standard error with `eprintln`, and the function
  returns a failing `ExitCode`.
- **In a package**, `hd.toml` must grant what `main` names:
  `Args = true`, `Console = true`, `FsRead = true`
  ([cli/packages.md](../cli/packages.md)).
- **Subcommands**: `parser.command("add", sub_parser)`, then
  `parsed.command()` gives `(name, parsed)?`.
