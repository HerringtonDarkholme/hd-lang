# Fakes: Testing Code That Uses The Outside World

Code reaches the outside through the traits in its row, so a test installs
a replacement with `$.with(Trait=fake):` and needs no mocking library. A
unit test gets no real console, files or clock: anything the code under
test requires must be installed in the test.

## Standard fakes

| Capability | Fake | Read back with |
| --- | --- | --- |
| `Console` | `MemoryConsole::new()` from `std.console` | `console.output()`, `console.error_output()` |
| `ConsoleInput` | `ScriptedInput::new(lines)` from `std.console` | |
| `FsRead`, `FsWrite` | `MemoryFs::new()` from `std.fs` | `read_text!` through the same fake |
| `Clock` | `ManualClock::new(start)` from `std.time` | sleeps finish at once, in virtual time |
| `Random` | `SeededRandom::new(seed)` from `std.random` | |
| `Args` | `box(ScriptedArgs::new(program, values))` from `std.cli` | |
| `Env` | `box(ScriptedEnv::new({"HOME": "/h"}))` from `std.host` | |
| `Http` | `ScriptedHttp::new()` from `std.http`, then `reply(url, result)` | `http.received()` ([how-to/http-api.md](../how-to/http-api.md)) |
| `Process` | `ScriptedProcess::new()` from `std.process`, then `reply(program, result)` | `process.received()` ([how-to/run-commands.md](../how-to/run-commands.md)) |
| `Signals` | `ScriptedSignals::new([.Interrupt])` from `std.host` | |

Each `new` returns a shared object, ready to pass to `$.with`, except
`ScriptedArgs` and `ScriptedEnv`: they are plain values, so they are boxed
at the call. Reach for these before writing a fake of your own; write one
only for your own traits.

A unit test that reaches `println` without a console installed fails to
check: ``missing-requirement: this needs `$ Console`, which the runner
does not bind for this test case``. Install a `MemoryConsole`.

## Printing

```hd
use std.console.MemoryConsole

pub fn greet(name: string) -> void $ Console:
    println("Hello, $name")

pub fn main() -> void $ Console:
    greet("Ada")

tests:
    it("greets through a replacement console"):
        console := MemoryConsole::new()
        $.with(Console=console):
            greet("Grace")
        assert_equal(console.output().join(""), "Hello, Grace\n")
```

## Files

One `MemoryFs` can serve both reading and writing.

```hd
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, create_dir_all, read_text, write_text}
use std.path.Path

fn save_report!(name: string, total: i32) -> Result[void, FsError] $ FsWrite:
    create_dir_all!(Path("reports"))?
    write_text!(Path("reports/$name.txt"), "total: $total")

fn read_report!(name: string) -> string $ FsRead:
    read_text!(Path("reports/$name.txt")).unwrap_or("missing")

tests:
    it("writes and reads back a report"):
        fs := MemoryFs::new()
        $.with(FsWrite=fs, FsRead=fs):
            assert(save_report!("q4", 4500).is_ok())
            assert_equal(read_report!("q4"), "total: 4500")
            assert_equal(read_report!("q1"), "missing")
```

## Time

```hd
use std.time.{Clock, ManualClock, Timestamp, now, s, sleep}

fn expires!() -> string $ Clock:
    sleep!(90s)
    now().to_rfc3339()

tests:
    it("waits ninety seconds of virtual time"):
        clock := ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Clock=clock):
            assert_equal(expires!(), "1970-01-01T00:01:30Z")
```

## Your own traits

Write a small struct that implements the trait and records what it saw.
Declare it inside `tests:` so it is not part of the program. The fake is a
box, and the test keeps the reference to read it afterwards.

```hd
pub trait Mailer:
    fn send(&mut self, to: string, body: string) -> void

pub fn remind(users: List[string]) -> usize $ Mailer:
    mailer := $.use(Mailer)
    for user in users:
        mailer.send(user, "your invoice is due")
    users.len()

tests:
    struct FakeMailer:
        sent: List[string]

    impl Mailer for FakeMailer:
        fn send(&mut self, to: string, body: string) -> void:
            self.sent.push(to)

    it("mails every user"):
        fake := box(FakeMailer { sent: [] })
        $.with(Mailer=fake):
            assert_equal(remind(["a", "b"]), 2)
        assert_equal(fake.sent, ["a", "b"], reason="one mail each")
```

Common slips:

- `$.with(Mailer=FakeMailer { ... })` with a plain value is a
  `type-mismatch`: wrap it in `box(...)`.
- Boxing a variable, `box(fake)`, copies it: the test then reads the
  untouched original. Box at the declaration, as above.
- A fake that records calls changes itself, so the trait method takes
  `&mut self`.
