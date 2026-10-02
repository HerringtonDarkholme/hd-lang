# Host

Status: standard library specification draft.

This chapter defines `std.host`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability traits `Args` and `Env`;
- the helpers `args` and `env`.

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

See also: [Program Arguments](../cli/command-line.md#program-arguments),
[Entry Arguments](../lang/10-modules.md#entry-arguments).
