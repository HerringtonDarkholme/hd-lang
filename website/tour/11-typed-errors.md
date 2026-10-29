---
title: Typed errors
---

# Typed errors

The program on the right fails to read a port and prints why.

## `@error` types

```hd example
@error
enum ConfigError:
    @error("port is not a number")
    InvalidPort(@from error: ParseNumberError)
```

An `@error` type keeps failures structured. Each case gets a **display
message**.

## `@from`

```hd example
port := text.parse::[i32]()?
```

`@from` generates the conversion this `?` uses, from `ParseNumberError` to
`ConfigError`. It also records the parse error as the **cause**. Without
`@from`, `?` has no conversion.

## Context

```hd example
fn load(text: string) -> Result[i32, *dyn Error]:
    parse_port(text).context("loading server config")
```

`.context` adds **what the caller was doing**. `*dyn Error` is a shared
reference to any error type.

## Reports

```hd example
match load("eighty"):
    .Ok(port) => println("port: $port")
    .Err(error) => println(report_of(error))
```

`report_of` turns the **cause chain** into text for display or logging.

**Run it.** `"eighty"` is not a number. So it prints the context, then each
cause on its own `source:` line.

```hd
use std.error.{Error, report_of}
use std.num.ParseNumberError

@error
enum ConfigError:
    @error("port is not a number")
    InvalidPort(@from error: ParseNumberError)

fn parse_port(text: string) -> Result[i32, ConfigError]:
    port := text.parse::[i32]()?
    .Ok(port)

fn load(text: string) -> Result[i32, *dyn Error]:
    parse_port(text).context("loading server config")

match load("eighty"):
    .Ok(port) => println("port: $port")
    .Err(error) => println(report_of(error))
```

```edit
replace:     InvalidPort(@from error: ParseNumberError)
with:     InvalidPort(error: ParseNumberError)
error: invalid-result-propagation: ConfigError does not implement From[ParseNumberError]
```

```output
loading server config
source: port is not a number
source: invalid digit at position 0
```
