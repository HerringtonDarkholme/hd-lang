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
one thin module, and let `main` only read arguments and pick an exit
status.

```text
todo/
  hd.toml
  src/
    main.hd             arguments in, exit status out
    commands/mod.hd     one function per command; uses tasks and storage
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
use std.console.MemoryConsole
use std.fs.{FsRead, FsWrite, MemoryFs}
use std.path.Path

@error
pub enum CommandError:
    @error("usage: todo [add TITLE | done NUMBER | list]")
    Usage
    @error("task error")
    Task(@from error: TaskError)
    @error("storage error")
    Storage(@from error: StorageError)

pub fn run!(arguments: List[string]) -> Result[void, CommandError] $ Console + FsRead + FsWrite:
    file := Path("todo.json")
    tasks := load!(file)?
    match (arguments.get(0).unwrap_or("list"), arguments.get(1)):
        ("add", .Some(title)) =>
            save!(file, added(tasks, title)?)?
            println("added: $title")
        ("done", .Some(text)) =>
            let .Ok(number) = text.parse::[usize]() else: return .Err(.Usage)
            save!(file, completed(tasks, number)?)?
            println("done: $number")
        ("list", _) => println(listing(tasks))
        _ => return .Err(.Usage)
    .Ok(())

tests:
    it("adds, completes and lists through fakes"):
        fs := MemoryFs::new()
        console := MemoryConsole::new()
        $.with(FsRead=fs, FsWrite=fs, Console=console):
            assert(run!(["add", "buy milk"]).is_ok())
            assert(run!(["done", "1"]).is_ok())
            assert(run!([]).is_ok())
            assert(run!(["done", "7"]).is_err())
            assert(run!(["dance"]).is_err())
        assert_equal(console.output().join(""), "added: buy milk\ndone: 1\n1. [x] buy milk\n")
```

<!-- file: todo/src/main.hd -->
```hd
use pkg.commands.run
use std.console.eprintln
use std.fs.{FsRead, FsWrite}
use std.cli.{Args, args}
use std.process.ExitCode

pub fn main!() -> ExitCode $ Args + Console + FsRead + FsWrite:
    match run!(args()):
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
    let out = hd_run!(*Command::new("todo").arg("dance").current_dir(temp_dir()))
    assert_equal(out.status, 1)
    assert_equal(out.stderr, "todo: usage: todo [add TITLE | done NUMBER | list]\n")
```

## Work on it

```sh
hd check --tests            # every module and its tests
hd test                     # unit tests, then tests/todo.hd against the built program
hd run -- add "buy milk"    # arguments go after `--`
hd run -- list
hd fmt && hd lint
```

## The pieces

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
