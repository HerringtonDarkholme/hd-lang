---
title: Least authority in main
---

# `main`'s row is everything the program may touch

A program's `main` lists its requirements after `$` like any other function,
and that row is the most any code below it can reach. This one says
`$ Console`: it may print, and nothing else. No helper, however deep, and no
library, however updated, can open a file, call the network, or read the clock
behind its back, because a call that needs more than `main` grants does not
compile. To give the program more, you widen one line, where a reviewer sees
it.

```hd
pub fn main!() -> void $ Console:  # ← the console, and nothing else
    println(invoice_text(1042, 2449))

# Swap in the helper that also keeps a copy on disk: change the call
# to `invoice_text_saved!(1042, 2449)` and Run:
#     missing-requirement: call to 'invoice_text_saved' requires FsWrite

# ── plumbing ──
use std.fs.{FsWrite, write_text}
use std.path.Path

fn invoice_text(number: i32, cents: i32) -> string:
    "Invoice $number: $cents cents due"

fn invoice_text_saved!(number: i32, cents: i32) -> string $ FsWrite:
    text := invoice_text(number, cents)
    _ := write_text!(Path("invoice-$number.txt"), text)
    text
```

```edit
replace:     println(invoice_text(1042, 2449))
with:     println(invoice_text_saved!(1042, 2449))
error: missing-requirement: call to 'invoice_text_saved' requires FsWrite
```

```output
Invoice 1042: 2449 cents due
```
