# Path

Status: standard library specification draft.

This chapter defines `std.path`, which `lib/std` writes in ordinary hd
over the language tier:

- the `Path` type that the file system traits take.

## Paths

A `Path` is a newtype over the text of a file system path:

```text
use std.path.Path

fn notes() -> Path:
    Path("notes/today.txt")
```

1. r[std-path.decl] `std.path` declares `pub type Path(string)`. Code imports it, as in `use std.path.Path`.
2. r[std-path.text] A `Path` holds its text as written. Constructing one neither checks nor normalizes it.
3. r[std-path.traits] `Path` implements `Eq`, which compares the text, and `Display`, which shows the text.

> **Why.** Go's `path/filepath` works on plain strings. A newtype keeps
> that cost and still tells a path from other text in a signature.

See also: [Fs](fs.md).
