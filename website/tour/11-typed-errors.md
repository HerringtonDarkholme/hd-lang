---
title: Typed errors
---

# `@error` gives failures messages and causes, while context records the operation

An `@error` type keeps failures structured and gives each case a display
message. `@from` generates the conversion used by `?` and records the converted
error as the cause. `.context` adds what the caller was doing. `report_of`
turns the resulting chain into a boundary-safe value for display or logging.

```hd
use std.error.{Error, report_of}
use std.num.ParseNumberError

@error
enum ConfigError:
    @error("port is not a number")
    InvalidPort(
        @from
        error: ParseNumberError,
    )

fn parse_port(text: string) -> Result[i32, ConfigError]:
    port := text.parse::[i32]()?
    .Ok(port)

fn load(text: string) -> Result[i32, *dyn Error]:
    parse_port(text).context("loading server config")

match load("eighty"):
    .Ok(port) => println("port: $port")
    .Err(error) => println(report_of(error))

# Delete `@from` and Run. Then `?` has no conversion:
#     invalid-result-propagation: ConfigError does not implement From[ParseNumberError]
```

```edit
replace:         @from
with:
error: invalid-result-propagation: ConfigError does not implement From[ParseNumberError]
```

```output
loading server config
source: port is not a number
source: invalid digit at position 0
```
