---
title: Hello, hd
---

# A program starts at `main`, and `main` says what it may touch

This tour runs hd in your browser. Press **Run**, or Ctrl+Enter (Cmd+Enter on
a Mac), to compile and run the code on the right. Edit it freely: your changes
stay in this browser, and **Reset** brings back the original.

`$ Console` after the signature is a requirement. It is the whole list of
outside things this program may use: it may print, and nothing else. No library
it calls can quietly read your files or open a socket unless `main` grants it,
so a reviewer learns the program's reach from one line.

```hd
# Press Run, or Ctrl+Enter. Then change the name and run it again.
pub fn main() -> void $ Console:  # ← may print, and nothing else
    customer := "Ada"
    println("Hello, $customer! Your order has shipped.")

# Delete `$ Console` from main's signature and Run:
#     missing-requirement: call to 'println' requires Console
```

```edit
replace: pub fn main() -> void $ Console:  # ← may print, and nothing else
with: pub fn main() -> void:
error: missing-requirement: call to 'println' requires Console
```

```output
Hello, Ada! Your order has shipped.
```
