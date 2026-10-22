# Host

Status: standard library specification draft.

This chapter defines `std.host`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability traits `Args`, `Env`, and `Sys`;
- the helpers `args`, `env`, and `env_as`, and the shared `HostError`;
- `ScriptedArgs`, `ScriptedEnv`, and `ScriptedSys`, their deterministic providers;
- what every host operation does while it waits, when it is cancelled,
  and with the data it holds.

Which provider a command binds for each trait is CLI tier
([Host Capabilities](../cli/command-line.md#host-capabilities)).

## Program Arguments

`Args` reads the arguments the program was started with:

```text
pub trait Args:
    fn program(self) -> string
    fn list(self) -> List[string]
```

1. r[std-host.args.decl] `std.host` declares the host capability trait `Args` with the methods above. Code imports it, as in `use std.host.Args`.
2. r[std-host.args.program] `program` returns the name the program was started with.
3. r[std-host.args.list] `list` returns the program's arguments, in order, without the program name.
4. r[std-host.args.helper] `std.host` declares `pub fn args() -> List[string] $ Args`, which returns `list()` of the `Args` provider that covers the call.

## Environment

`Env` reads environment variables:

```text
pub trait Env:
    fn get(self, name: string) -> string?
    fn names(self) -> List[string]
```

1. r[std-host.env.decl] `std.host` declares the host capability trait `Env` with the methods above. Code imports it, as in `use std.host.Env`.
2. r[std-host.env.get] `get(name)` returns the value of the variable `name`, or `.None` when it is not set.
3. r[std-host.env.names] `names` returns the name of every set variable, each once.
4. r[std-host.env.helper] `std.host` declares `pub fn env(name: string) -> string? $ Env`, which returns `get(name)` of the `Env` provider that covers the call.

> **Note.** Under an `Env` grant, the default profile's `get` returns
> `.None` for a variable outside the grant, and `names` leaves it out
> ([Environment Grant](../cli/command-line.md#environment-grant)).

### Typed Variables

`env_as` reads a variable as any type that implements
[`Parse`](text.md#parsing-text):

```text
use std.host.{Env, HostError, env_as}

fn workers() -> Result[usize, HostError] $ Env:
    .Ok(env_as::[usize]("WORKERS")?.unwrap_or(4))
```

```text
pub enum HostError:
    NotGranted(subject: string)
    Unsupported(name: string)
    Other(message: string)
    InvalidValue(name: string, reason: string)
```

1. r[std-host.env-as.shared-error-signature] `std.host` declares `pub fn env_as[T < Parse[Error = E], E < Display](name: string) -> Result[T?, HostError] $ Env`. The call names the type, as in `env_as::[usize]("WORKERS")`.
2. r[std-host.env-as.unset] When `env(name)` is `.None`, `env_as` returns `.Ok(.None)`.
3. r[std-host.env-as.present] When `env(name)` is `.Some(text)` and `T::parse(text)` returns `.Ok(value)`, `env_as` returns `.Ok(.Some(value))`.
4. r[std-host.env-as.invalid-host-error] When `T::parse(text)` returns `.Err(error)`, `env_as` returns `.Err(HostError.InvalidValue(name, reason))`. `reason` is the `Display` text of `error`.
5. r[std-host.error.decl] `std.host` declares `HostError` with the four variants above.
6. r[std-host.env-as.plain] `env_as` is a plain call, like `env`.
7. r[std-host.error.traits] `HostError` implements `Eq`, `Debug`, `Display`, and `Error`.
8. r[std-host.error.invalid-value-display] `InvalidValue(name, reason)` displays `invalid value for NAME: REASON`.
9. r[std-host.error.not-granted-display] `NotGranted(subject)` displays `sys access to SUBJECT is not granted`.
10. r[std-host.error.unsupported-display] `Unsupported(name)` displays `sys access to NAME is not supported`.
11. r[std-host.error.other-display] `Other(message)` displays `message` unchanged.

> **Why.** An unset variable is normal, so it is `.Ok(.None)`, as Go's
> `os.LookupEnv` reports it apart from an empty value. A set variable that
> does not parse is a mistake the program should show, so it is an error
> that names the variable, as `strconv` errors name the text.

## Plain Reads

1. r[std-host.plain-reads] Every method of `Args` and `Env`, and both helpers, are plain calls, not bang calls.

```text
use std.host.{Args, Env, args, env}

fn greeting() -> string $ Args + Env:
    let names = args()
    match env("GREETING"):
        .Some(word) => "${word}, ${names.len()}"
        .None => "hello"
```

> **Why.** An argument or environment read returns a value the host
> already holds, so it needs no driver. Replay records it at the
> boundary either way.

## Map Providers

`ScriptedArgs` and `ScriptedEnv` are the deterministic providers of `Args` and
`Env`. A test supplies them with `$.with`:

```text
use std.host.{Args, Env, ScriptedArgs, ScriptedEnv, args, env}
fn greeting() -> string $ Args + Env:
    match env("GREETING"):
        .Some(word) => "${word}, ${args().len()}"
        .None => "hello"

tests:
    use std.testing.assert_equal

    it("greets from fixed input"):
        given := ScriptedArgs::new("tool", ["a", "b"])
        vars := ScriptedEnv::new({"GREETING": "hi", "HOME": "/home/me"})
        $.with(Args=given, Env=vars):
            assert_equal(greeting(), "hi, 2", reason="one variable, two arguments")
        assert_equal(vars.names().sorted(), ["GREETING", "HOME"], reason="both keys")
```

1. r[std-host.map-args.decl] `std.host` declares `ScriptedArgs`, which implements `Args`, with private fields. Code imports it, as in `use std.host.ScriptedArgs`.
2. r[std-host.map-args.new] `ScriptedArgs::new(program: string, values: List[string]) -> ScriptedArgs` returns a provider whose `program` returns `program` and whose `list` returns `values`.
3. r[std-host.map-env.decl] `std.host` declares `ScriptedEnv`, which implements `Env`, with private fields. Code imports it, as in `use std.host.ScriptedEnv`.
4. r[std-host.map-env.new] `ScriptedEnv::new(values: Map[string, string]) -> ScriptedEnv` returns a provider whose `get(name)` returns the value of the key `name` in `values`, or `.None` when there is no such key.
5. r[std-host.map-env.names.order] `names` on a `ScriptedEnv` returns the keys of `values` in the map's [iteration order](../lang/04-type-system.md#r-types.map.order.deterministic).
6. r[std-host.map.no-host] Neither provider reads the host's arguments or environment.

> **Why.** A test states every argument and variable that the code under
> test reads, so nothing leaks in from the machine that runs it. Effect's
> `ConfigProvider.fromMap` serves tests the same way.

## System Reads

`Sys` reads four facts about the host, as Go's `runtime.GOOS`,
`runtime.GOARCH`, `runtime.NumCPU`, and `os.Hostname` do:

```text
pub trait Sys:
    fn os(self) -> Result[string, HostError]
    fn arch(self) -> Result[string, HostError]
    fn hostname(self) -> Result[string, HostError]
    fn cpu_count(self) -> Result[u32, HostError]
```

1. r[std-host.sys.decl] `std.host` declares the host capability trait `Sys` as above. Code imports it, as in `use std.host.Sys`.
2. r[std-host.sys.os] `os` returns the operating system's name in lowercase, such as `linux`, `macos`, or `windows`.
3. r[std-host.sys.arch] `arch` returns the host machine's architecture, such as `x86_64` or `aarch64`.
4. r[std-host.sys.hostname] `hostname` returns the host's name.
5. r[std-host.sys.cpu-count] `cpu_count` returns the number of logical processors that the host offers the program.
6. r[std-host.sys.plain] Every method of `Sys` is a plain call, not a bang call, as the reads of `Env` are by [`std-host.plain-reads`](#r-std-host.plain-reads).
7. r[std-host.sys.not-granted] A method returns `.Err(HostError.NotGranted(subject))` when the program's capability grant does not cover it, with the method's name for `subject`.
8. r[std-host.sys.unsupported] A method returns `.Err(HostError.Unsupported(name))` when the host can't answer it, as a browser can't for `hostname`.

> **Why.** The grant covers each read by its method's name, as Deno gates
> each `sys` call, so each method returns a `Result`.

## Map Sys

`ScriptedSys` is the deterministic `Sys` provider. A test supplies it with
`$.with`:

```text
use std.host.{ScriptedSys, Sys, HostError}
fn platform() -> string $ Sys:
    match $.use(Sys).os():
        .Ok(name) => name
        .Err(_) => "unknown"

tests:
    use std.testing.assert_equal

    it("reads the mapped system"):
        $.with(Sys=ScriptedSys::new({"os": "linux", "cpu_count": "8"})):
            assert_equal(platform(), "linux", reason="the mapped os")
            assert_equal($.use(Sys).cpu_count(), .Ok(8), reason="parsed as a count")
            assert_equal($.use(Sys).hostname(), .Err(HostError.Unsupported("hostname")), reason="not in the map")
```

1. r[std-host.sys.map.decl] `std.host` declares `ScriptedSys`, which implements `Sys`, with private fields. Code imports it, as in `use std.host.ScriptedSys`.
2. r[std-host.sys.map.new] `ScriptedSys::new(values: Map[string, string]) -> ScriptedSys` returns a provider whose methods each return the value of the key that is the method's name.
3. r[std-host.sys.map.cpu-count] `cpu_count` reads its value as a decimal `u32`.
4. r[std-host.sys.map.missing] A method whose key is missing, or whose value `cpu_count` can't read, returns `.Err(HostError.Unsupported(name))`.
5. r[std-host.sys.map.no-host] A `ScriptedSys` never reads the host system.

## Host Operations

A host operation is a bang method call that the host completes, such as
`send!` on the host's `Http` provider. While one waits, the instance runs
its other tasks:

```hd
use std.http.{Http, HttpError, Response, get}
use std.task.all

fn both!(left: string, right: string) -> (Result[Response, HttpError], Result[Response, HttpError]) $ Http:
    all!(get(left), get(right))   # both requests wait at the same time
```

1. r[std-host.op.wait] A bang host method that waits lets the instance run its other tasks until it completes.
2. r[std-host.op.no-order] Independent host operations that complete together have no defined order, so the instance may resume their tasks in any order.
3. r[std-host.op.cancel] Cancelling the suspension of a pending host operation ends that operation. Each trait states what the operation leaves behind, as the table below lists.
4. r[std-host.op.cancel.write] A cancelled host operation that writes may have happened, in whole or in part.

| Trait | A cancelled operation leaves |
| --- | --- |
| `Process` | no child: it and its process group are killed, by [`std-process.cancel`](process.md#r-std-process.cancel) |
| `Http` | no connection: it is closed, by [`std-http.send.cancel.close`](http.md#r-std-http.send.cancel.close) |
| `Net` | the bytes not yet returned, for the next read, by [`std-net.cancel.read`](net.md#r-std-net.cancel.read) |
| `FsWrite` | possibly a partial file, by [`std-fs.cancel.write`](fs.md#r-std-fs.cancel.write) |

> **Why.** A `race!` timeout around a host call is portable only when
> the call lets the timer run. Completions that arrive together may
> reach the instance in one batch, so no program may rely on their order.

See also: [Program Arguments](../cli/command-line.md#program-arguments),
[Entry Arguments](../lang/10-modules.md#entry-arguments).
