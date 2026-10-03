# Host

Status: standard library specification draft.

This chapter defines `std.host`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability traits `Args` and `Env`;
- the helpers `args` and `env`;
- `MapArgs` and `MapEnv`, their deterministic providers.

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

`MapArgs` and `MapEnv` are the deterministic providers of `Args` and
`Env`. A test supplies them with `$.with`:

```text
use std.host.{Args, Env, MapArgs, MapEnv, args, env}
use std.testing.assert_equal

fn greeting() -> string $ Args + Env:
    match env("GREETING"):
        .Some(word) => "${word}, ${args().len()}"
        .None => "hello"

tests:
    it("greets from fixed input"):
        given := MapArgs::new("tool", ["a", "b"])
        vars := MapEnv::new({"GREETING": "hi", "HOME": "/home/me"})
        $.with(Args=given, Env=vars):
            assert_equal(greeting(), "hi, 2", reason="one variable, two arguments")
        assert_equal(vars.names(), ["GREETING", "HOME"], reason="insertion order")
```

1. r[std-host.map-args.decl] `std.host` declares `MapArgs`, which implements `Args`, with private fields. Code imports it, as in `use std.host.MapArgs`.
2. r[std-host.map-args.new] `MapArgs::new(program: string, values: List[string]) -> MapArgs` returns a provider whose `program` returns `program` and whose `list` returns `values`.
3. r[std-host.map-env.decl] `std.host` declares `MapEnv`, which implements `Env`, with private fields. Code imports it, as in `use std.host.MapEnv`.
4. r[std-host.map-env.new] `MapEnv::new(values: Map[string, string]) -> MapEnv` returns a provider whose `get(name)` returns the value of the key `name` in `values`, or `.None` when there is no such key.
5. r[std-host.map-env.names] `names` on a `MapEnv` returns the keys of `values` in insertion order.
6. r[std-host.map.no-host] Neither provider reads the host's arguments or environment.

> **Why.** A test states every argument and variable that the code under
> test reads, so nothing leaks in from the machine that runs it. Effect's
> `ConfigProvider.fromMap` serves tests the same way.

See also: [Program Arguments](../cli/command-line.md#program-arguments),
[Entry Arguments](../lang/10-modules.md#entry-arguments).
