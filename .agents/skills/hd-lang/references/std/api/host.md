# `std.host`

Generated from `hd doc std.host`. Run `hd doc std.host.NAME` in a package for one item's full text.

## `Signal`

```text
pub enum Signal
```

A portable operating-system notification.

## `Signals`

```text
pub trait Signals
```

The host capability to await the next notification of one kind.

## `ScriptedSignals`

```text
pub struct ScriptedSignals
```

The deterministic Signals provider: each kind has its own scripted supply.

- `pub fn new(script: List[Signal]) -> *mut ScriptedSignals`: Returns a provider that consumes the supplied signals by requested kind.

## `Env`

```text
pub trait Env
```

The host capability to read the process's environment variables.

## `env`

```text
pub fn env(name: string) -> string? $ Env
```

The value of the environment variable `name`, in `.Some`, or `.None` when it is not set.

## `env_as`

```text
pub fn env_as[T < Parse[Error = E], E < Display](name: string) -> Result[T?, HostError] $ Env
```

The value of the environment variable `name` read as a `T`: `.None` when unset, an error naming the variable when unreadable.

## `env_decode`

```text
pub fn env_decode[T < Deserialize](prefix: string = "") -> Result[T, HostError] $ Env
```

Reads a record from its declared environment variables through Deserialize.

## `HostError`

```text
pub enum HostError
```

Why a host read failed, including a variable that could not be parsed.

## `ScriptedEnv`

```text
pub struct ScriptedEnv
```

The deterministic `Env` provider: a fixed map of variables, with no host access.

- `pub fn new(values: Map[string, string]) -> ScriptedEnv`: Returns the provider that answers from `values`.

## `Sys`

```text
pub trait Sys
```

The host capability to read four facts about the host system.

## `ScriptedSys`

```text
pub struct ScriptedSys
```

The deterministic Sys provider: every read answers from a map.

- `pub fn new(values: Map[string, string]) -> ScriptedSys`: Returns the provider whose methods read the value of the key that is the method's name.
