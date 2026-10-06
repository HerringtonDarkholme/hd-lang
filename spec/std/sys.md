# Sys

Status: standard library specification draft.

This chapter defines `std.sys`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability trait `Sys`, four reads about the host system;
- the error enum `SysError`;
- `MapSys`, the deterministic provider of `Sys`.

Which provider a command binds, and which methods its grant covers, is CLI
tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).

## System Reads

`Sys` reads four facts about the host, as Go's `runtime.GOOS`,
`runtime.GOARCH`, `runtime.NumCPU`, and `os.Hostname` do:

```text
pub enum SysError:
    NotGranted(name: string)
    Unsupported(name: string)

pub trait Sys:
    fn os(self) -> Result[string, SysError]
    fn arch(self) -> Result[string, SysError]
    fn hostname(self) -> Result[string, SysError]
    fn cpu_count(self) -> Result[u32, SysError]
```

1. r[std-sys.decl] `std.sys` declares `SysError` and the host capability trait `Sys` as above. Code imports them, as in `use std.sys.{Sys, SysError}`.
2. r[std-sys.os] `os` returns the operating system's name in lowercase, such as `linux`, `macos`, or `windows`.
3. r[std-sys.arch] `arch` returns the host machine's architecture, such as `x86_64` or `aarch64`.
4. r[std-sys.hostname] `hostname` returns the host's name.
5. r[std-sys.cpu-count] `cpu_count` returns the number of logical processors that the host offers the program.
6. r[std-sys.plain] Every method of `Sys` is a plain call, not a bang call, as the reads of `Env` are by [`std-host.plain-reads`](host.md#r-std-host.plain-reads).
7. r[std-sys.not-granted] A method returns `.Err(SysError.NotGranted(name))` when the program's capability grant does not cover it, with the method's name for `name`.
8. r[std-sys.unsupported] A method returns `.Err(SysError.Unsupported(name))` when the host can't answer it, as a browser can't for `hostname`.
9. r[std-sys.error.traits] `SysError` implements `Eq`, `Debug`, `Display`, and `std.error.Error`.
10. r[std-sys.error.not-granted.text] The `Display` text of `NotGranted(name)` is `sys access to NAME is not granted; run with --cap Sys=NAME`, with the method's name for `NAME`.

> **Why.** The grant covers each read by its method's name, as Deno gates
> each `sys` call, so each method returns a `Result`.

## Map Sys

`MapSys` is the deterministic `Sys` provider. A test supplies it with
`$.with`:

```text
use std.sys.{MapSys, Sys, SysError}
use std.testing.assert_equal

fn platform() -> string $ Sys:
    match $.use(Sys).os():
        .Ok(name) => name
        .Err(_) => "unknown"

tests:
    it("reads the mapped system"):
        $.with(Sys=MapSys::new({"os": "linux", "cpu_count": "8"})):
            assert_equal(platform(), "linux", reason="the mapped os")
            assert_equal($.use(Sys).cpu_count(), .Ok(8), reason="parsed as a count")
            assert_equal($.use(Sys).hostname(), .Err(SysError.Unsupported("hostname")), reason="not in the map")
```

1. r[std-sys.map.decl] `std.sys` declares `MapSys`, which implements `Sys`, with private fields. Code imports it, as in `use std.sys.MapSys`.
2. r[std-sys.map.new] `MapSys::new(values: Map[string, string]) -> MapSys` returns a provider whose methods each return the value of the key that is the method's name.
3. r[std-sys.map.cpu-count] `cpu_count` reads its value as a decimal `u32`.
4. r[std-sys.map.missing] A method whose key is missing, or whose value `cpu_count` can't read, returns `.Err(SysError.Unsupported(name))`.
5. r[std-sys.map.no-host] A `MapSys` never reads the host system.

See also: [Host](host.md), [Capability Grants](../cli/command-line.md#capability-grants).
