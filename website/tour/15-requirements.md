---
title: Requirements
---

# Requirements name outside needs, providers replace them, and `main` sets the limit

A function lists outside needs after `$`. `greet` may write to `Console` and
nothing else. `$.with` installs a replacement provider for one block, so the
test records output without a real console. An explicit `main` passes the real
provider through, and its row is the complete list of outside services any
called helper may use. This is the first lesson whose function body prints.

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

# Delete `$ Console` from `greet` and Run:
#     missing-requirement: this needs `$ Console`, which the enclosing function's row does not name
# Delete the `$.with` line, dedent `greet("Grace")`, and press Test:
#     missing-requirement: this needs `$ Console`, which the runner does not bind for this test case
# Change `greet("Ada")` in `main` to `save_receipt()` and Run:
#     missing-requirement: this needs `$ FsWrite`, which the enclosing function's row does not name
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
