# How To: Start And Structure A Project

**Scenario.** A to-do command grows past one file. It needs modules with
clear jobs, tests next to the code, a real run from the command line, and
the right grants in `hd.toml`.

## Start

```sh
hd new todo --app --vcs none    # leave `--vcs none` off to also run `git init`
cd todo
hd run                          # prints hello, world
hd test
```

## The shape to aim for

Put the rules in plain functions with no row, keep the outside world in
one thin module, and let `main` only parse the arguments and pick an exit
status. The arguments are a `@derive(Cli)` type, never a hand-matched
list of strings.

```text
todo/
  hd.toml
  src/
    main.hd             arguments in, exit status out
    commands/mod.hd     the commands as a `@derive(Cli)` enum, and what each does
    storage/mod.hd      load and save: the only module that touches files
    tasks/mod.hd        the types and rules: no `$`, no `!`
  tests/
    todo.hd             runs the built program
```

Each module imports only the ones below it, so no two folders import
each other (`folder-cycle`).

## The files

<!-- file: todo/hd.toml -->
```toml
[package]
name = "todo"

[capabilities]
Console = true
Args = true
FsRead = true           # the tool works on `todo.json` in whatever folder it runs in
FsWrite = true

[dev-capabilities]
Process = true          # the integration test runs the built program
```

<!-- file: todo/src/tasks/mod.hd -->
```hd
use std.encoding.{Deserialize, Serialize}

@derive(Serialize, Deserialize, Eq, Debug)
pub struct Task:
    pub title: string
    pub done: bool = false

@error
@derive(Eq, Debug)
pub enum TaskError:
    @error("a task needs a title")
    EmptyTitle
    @error("there is no task $number")
    NoSuchTask(number: usize)

pub fn added(tasks: List[Task], title: string) -> Result[List[Task], TaskError]:
    if title.trim() == "":
        return .Err(.EmptyTitle)
    .Ok([tasks..., Task { title: title.trim() }])

# `number` counts from 1, as the listing shows it.
pub fn completed(tasks: List[Task], number: usize) -> Result[List[Task], TaskError]:
    if number == 0 || number > tasks.len():
        return .Err(.NoSuchTask(number))
    let mut result = tasks
    result[number - 1].done = true
    .Ok(result)

pub fn listing(tasks: List[Task]) -> string:
    if tasks.is_empty():
        return "nothing to do"
    lines := [for (index, task) in tasks.iter().enumerate()
              => "${index + 1}. [${if task.done: "x" else: " "}] ${task.title}"]
    lines.join("\n")

tests:
    it("adds a trimmed task and refuses an empty one"):
        assert_equal(added([], "  buy milk "), .Ok([Task { title: "buy milk" }]))
        assert_equal(added([], "  "), .Err(.EmptyTitle))

    it("completes by number"):
        tasks := [Task { title: "a" }, Task { title: "b" }]
        assert_equal(completed(tasks, 2), .Ok([Task { title: "a" }, Task { title: "b", done: true }]))
        assert_equal(completed(tasks, 3), .Err(.NoSuchTask(3)))

    it("lists with numbers and marks"):
        assert_equal(listing([Task { title: "a", done: true }, Task { title: "b" }]), "1. [x] a\n2. [ ] b")
        assert_equal(listing([]), "nothing to do")
```

<!-- file: todo/src/storage/mod.hd -->
```hd
use pkg.tasks.Task
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, read_text, stat, write_text}
use std.encoding.json.{JsonError, decode, encode_pretty}
use std.path.Path

@error
pub enum StorageError:
    @error("cannot use the task file")
    File(@from error: FsError)
    @error("the task file is damaged")
    Format(@from error: JsonError)

pub fn load!(path: Path) -> Result[List[Task], StorageError] $ FsRead:
    if stat!(path)?.is_none():
        return .Ok([])                      # first run: no file yet
    .Ok(decode::[List[Task]](read_text!(path)?)?)

pub fn save!(path: Path, tasks: List[Task]) -> Result[void, StorageError] $ FsWrite:
    write_text!(path, encode_pretty(tasks))?
    .Ok(())

tests:
    it("starts empty, then loads what it saved"):
        fs := MemoryFs::new()
        $.with(FsRead=fs, FsWrite=fs):
            assert_equal(load!(Path("todo.json")).ok(), .Some([]))
            assert(save!(Path("todo.json"), [Task { title: "a" }]).is_ok())
            assert_equal(load!(Path("todo.json")).ok(), .Some([Task { title: "a" }]))
```

<!-- file: todo/src/commands/mod.hd -->
```hd
use pkg.storage.{StorageError, load, save}
use pkg.tasks.{TaskError, added, completed, listing}
use std.cli.{Cli, positional}
use std.console.MemoryConsole
use std.fs.{FsRead, FsWrite, MemoryFs}
use std.path.Path

## Keep a to-do list in todo.json
@derive(Cli, Eq, Debug)
pub struct Todo:
    pub command: Command = .List

@derive(Cli, Eq, Debug)
pub enum Command:
    ## add a task
    Add(AddOptions)
    ## mark a task as done
    Done(DoneOptions)
    ## print every task
    List

@derive(Cli, Eq, Debug)
pub struct AddOptions:
    ## what to do
    @positional
    pub title: string

@derive(Cli, Eq, Debug)
pub struct DoneOptions:
    ## the task's number in the listing
    @positional
    pub number: usize

@error
pub enum CommandError:
    @error("$error")
    Task(@from error: TaskError)
    @error("$error")
    Storage(@from error: StorageError)

pub fn run!(command: Command) -> Result[void, CommandError] $ Console + FsRead + FsWrite:
    file := Path("todo.json")
    tasks := load!(file)?
    match command:
        .Add(options) =>
            save!(file, added(tasks, options.title)?)?
            println("added: ${options.title}")
        .Done(options) =>
            save!(file, completed(tasks, options.number)?)?
            println("done: ${options.number}")
        .List => println(listing(tasks))
    .Ok(())

tests:
    it("parses each command"):
        assert_equal(Todo::parse(["add", "buy milk"]).ok(), .Some(Todo { command: .Add(AddOptions { title: "buy milk" }) }))
        assert_equal(Todo::parse([]).ok(), .Some(Todo { command: .List }))
        assert(Todo::parse(["done", "two"]).is_err())

    it("adds, completes and lists through fakes"):
        fs := MemoryFs::new()
        console := MemoryConsole::new()
        $.with(FsRead=fs, FsWrite=fs, Console=console):
            assert(run!(.Add(AddOptions { title: "buy milk" })).is_ok())
            assert(run!(.Done(DoneOptions { number: 1 })).is_ok())
            assert(run!(.List).is_ok())
            assert(run!(.Done(DoneOptions { number: 7 })).is_err())
        assert_equal(console.output().join(""), "added: buy milk\ndone: 1\n1. [x] buy milk\n")
```

<!-- file: todo/src/main.hd -->
```hd
use pkg.commands.{Todo, run}
use std.cli.{Args, Cli}
use std.console.eprintln
use std.fs.{FsRead, FsWrite}
use std.process.ExitCode

pub fn main!() -> ExitCode $ Args + Console + FsRead + FsWrite:
    todo := match Todo::parse_args():
        .Ok(todo) => todo
        .Err(error) => return error.exit()       # usage: status 2; --help: status 0
    match run!(todo.command):
        .Ok(_) => ExitCode(0)
        .Err(error) =>
            eprintln("todo: $error")
            ExitCode(1)
```

<!-- file: todo/tests/todo.hd -->
```hd
use std.process.Command
use std.testing.{hd_run, temp_dir}

it("keeps tasks between runs"):
    dir := temp_dir()                       # a fresh folder for this case
    let added = hd_run!(*Command::new("todo").args(["add", "buy milk"]).current_dir(dir))
    assert_equal(added.stdout, "added: buy milk\n")
    let listed = hd_run!(*Command::new("todo").current_dir(dir))
    assert_equal(listed.stdout, "1. [ ] buy milk\n")

it("fails with a message and status 1"):
    let out = hd_run!(*Command::new("todo").args(["done", "7"]).current_dir(temp_dir()))
    assert_equal(out.status, 1)
    assert_equal(out.stderr, "todo: there is no task 7\n")

it("refuses an unknown command with status 2"):
    let out = hd_run!(*Command::new("todo").arg("dance").current_dir(temp_dir()))
    assert_equal(out.status, 2)
```

## Work on it

```sh
hd check --tests            # every module and its tests
hd test                     # unit tests, then tests/todo.hd against the built program
hd run -- add "buy milk"    # arguments go after `--`
hd run -- list
hd run -- --help            # the generated help
hd fmt && hd lint
```

`hd run -- --help` prints the help derived from the types and their `##`
comments:

```text
usage: todo [options] [<command>]

Keep a to-do list in todo.json

commands:
  add   add a task
  done  mark a task as done
  list  print every task

options:
  -h, --help  Print this help.
```

## The pieces

- **Subcommands are an enum.** `@derive(Cli)` on `Command`, whose
  variants carry one `Cli` struct each (or none), reads `todo add TITLE`.
  The field `command: Command = .List` makes `todo` alone list. Each
  variant's `##` comment is its line in the help, and `todo add --help`
  prints that command's own help. An unknown command or a bad value exits
  with status 2 through `error.exit()`. `Todo::parse([...])` tests the
  parser without a host ([cli-tool.md](cli-tool.md)).

- **A folder of modules has `mod.hd`.** `src/tasks/mod.hd` is
  `pkg.tasks`. A lone file `src/tasks.hd` is the same module name, but it
  then lives in the same folder as `main.hd`, and a sub-folder that
  imports it makes a `folder-cycle`.
- **`pub` on everything another module uses**: the function, the types in
  its signature, and the fields it reads.
- **Errors climb by `@from`.** Each layer has its own `@error` enum that
  wraps the one below, so `?` works at every level and `main` prints one
  message.
- **Unit tests use fakes** (`MemoryFs`, `MemoryConsole`) and live in each
  module. **The integration test** runs the real program in a temporary
  folder, which needs `Process` under `[dev-capabilities]`.
- **Grants.** `hd.toml` lists every capability in `main`'s row. A list
  narrows one, as `FsRead = ["data/"]`, and its paths are resolved from
  the package folder. A program that reads only its own data should use a
  list. This tool works in the user's current folder, so it grants `true`:
  with `FsRead = ["todo.json"]`, running it anywhere but the package
  folder fails with `FsError.NotGranted`.
