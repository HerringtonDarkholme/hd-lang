---
title: Error enums and context
---

# `.context` turns a low-level failure into a report that says what you were doing

"invalid digit at position 0" is true, but it does not tell anyone what broke.
In hd an error type is an enum marked `@error`: each variant carries its data
and its message, and `@source` keeps the lower-level error it wraps.
`.context("...")` adds what the program was doing on the way up, and
`report_of` prints the whole chain, outermost first. Callers can still match
on the variants, because the error is a typed value, not a string. Delete the
`.context(...)` call and Run: each report loses the line that says what failed.

```hd
fn start(config: string) -> Result[i32, dyn Error]:
    port := read_port(config).context("starting the server")?  # ← what we were doing
    .Ok(port)

pub fn main() -> void $ Console:
    for config in ["port = 8080", "host = local", "port = eighty"]:
        match start(config):
            .Ok(port) => println("listening on $port")
            .Err(error) => println(report_of(error))  # ← the whole chain

# ── plumbing ──
use std.error.{Error, report_of}
use std.num.{ParseNumberError, parse_i32}

@error
enum ConfigError:
    @error("no 'port' line")
    MissingPort
    @error("port '$text' is not a number")
    BadPort(text: string, @source error: ParseNumberError)

fn read_port(config: string) -> Result[i32, ConfigError]:
    match config.split_once("="):
        .Some((key, value)) if key.trim() == "port" =>
            text := value.trim()
            parse_i32(text).map_err(fn(error: ParseNumberError) -> ConfigError: ConfigError.BadPort(text, error))
        _ => .Err(ConfigError.MissingPort)
```

```output
listening on 8080
starting the server
caused by: no 'port' line
starting the server
caused by: port 'eighty' is not a number
caused by: invalid digit at position 0
```
