---
title: Requirements
---

# Requirements name outside needs, providers replace them, and `main` caps authority

A function lists outside needs after `$`. `greet` may write to `Console` and
nothing else. `$.with` installs a replacement provider for one block, so the
test records output without a real console. An explicit `main` passes the real
provider through, and its row is the most authority any called helper may ask
for. This is the first lesson whose function body prints.

```hd
use std.console.MemoryConsole

pub fn greet(name: string) -> void $ Console:
    println("Hello, $name")

pub fn main() -> void $ Console:
    greet("Ada")

tests:
    it("greets through a replacement console"):
        console := MemoryConsole::new()
        $.with(Console=console):
            greet("Grace")
        assert_equal(console.output(), ["Hello, Grace", "\n"], reason="two writes")

# Delete `$ Console` from `greet` and Run:
#     missing-requirement: this needs `$ Console`, which the enclosing function's row does not name
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
