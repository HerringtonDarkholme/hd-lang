---
title: Requirements
---

# Requirements

The program on the right greets someone through the console, and its test
captures the greeting.

## Requirement rows

```hd example
pub fn greet(name: string) -> void $ Console:
    println("Hello, $name")
```

A function lists its outside needs after **`$`**. `greet` may write to
`Console` and nothing else. Earlier lessons printed only from a script's top
level, which needs no row.

## `main` sets the limit

```hd example
pub fn main() -> void $ Console:
    greet("Ada")
```

An explicit `main` passes the real provider down. Its row is the **complete
list** of outside services the program may use. `save_receipt` needs
`FsWrite`, so calling it from `main` is an error.

## Replacing a provider

```hd example
console := MemoryConsole::new()
$.with(Console=console):
    greet("Grace")
```

`$.with` installs a **replacement provider** for one block. So the test
records the output without a real console. Without `$.with`, the test runner
binds no `Console`, and the call is an error.

**Run it.** `main` greets Ada, so Run prints `Hello, Ada`. **Test** checks
the captured `Hello, Grace` and reports `1 test passed`.

```hd
use std.console.MemoryConsole
use std.fs.FsWrite

pub fn greet(name: string) -> void $ Console:
    println("Hello, $name")

fn save_receipt() -> void $ FsWrite:
    pass

pub fn main() -> void $ Console:
    greet("Ada")

tests:
    it("greets through a replacement console"):
        console := MemoryConsole::new()
        $.with(Console=console):
            greet("Grace")
        assert_equal(console.output().join(""), "Hello, Grace\n")
```

```edit
replace: pub fn greet(name: string) -> void $ Console:
with: pub fn greet(name: string) -> void:
error: missing-requirement: this needs `$ Console`, which the enclosing function's row does not name
```

```output
Hello, Ada
```

```tests
1 test passed
```
