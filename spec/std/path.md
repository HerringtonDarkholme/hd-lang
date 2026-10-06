# Path

Status: standard library specification draft.

This chapter defines `std.path`, which `lib/std` writes in ordinary hd
over the language tier:

- the `Path` type that the file system traits take;
- the text operations `join`, `parent`, `file_name`, and `extension`.

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

## Path Operations

A `Path` splits and joins its text at `/`, the only separator:

```text
use std.path.Path

fn config_file(home: Path) -> Path:
    home.join(".config").join("hd.toml")

fn sibling(file: Path, name: string) -> Path:
    match file.parent():
        .Some(dir) => dir.join(name)
        .None => Path(name)

fn is_markdown(file: Path) -> bool:
    match file.extension():
        .Some(text) => text == "md"
        .None => false
```

| Rule | Method | Result |
| --- | --- | --- |
| r[std-path.join] `join` | `pub fn join(self, part: string) -> Path` | the path followed by `part` |
| r[std-path.parent] `parent` | `pub fn parent(self) -> Path?` | the path without its last component, or `.None` when it has none |
| r[std-path.file-name] `file_name` | `pub fn file_name(self) -> string?` | the last component, when it names a file or directory |
| r[std-path.extension] `extension` | `pub fn extension(self) -> string?` | the text after the last `.` of the file name |

1. r[std-path.ops.methods] `std.path` gives `Path` the four methods above. Calling one needs only the `Path` import.
2. r[std-path.ops.separator] The separator is `/` on every host.
3. r[std-path.ops.components] The components of a path are the pieces of its text between separators. Empty pieces and `.` pieces are left out, except a `.` that comes first in a path that does not start with `/`.
4. r[std-path.ops.text-only] The methods read text only. None touches a file system, resolves `..`, or changes a path beyond what a rule below says.
5. r[std-path.join.absolute] When `part` starts with `/`, `join` returns `Path(part)`.
6. r[std-path.join.separator] Otherwise `join` returns the text of the path, then `/` unless that text is empty or ends in `/`, then `part`. So `Path("a").join("")` is `Path("a/")`.
7. r[std-path.parent.value] `parent` returns, in `.Some`, the path of every component but the last, separated by `/`, with a leading `/` when the path starts with `/`.
8. r[std-path.parent.none] A path with no component, `""` or `/`, has no parent, so `parent` returns `.None`.
9. r[std-path.parent.single] So a path of one component has the parent `Path("")`, or `Path("/")` when it starts with `/`.
10. r[std-path.file-name.value] `file_name` returns the last component in `.Some`. It returns `.None` when the path has no component or the last one is `.` or `..`.
11. r[std-path.extension.value] `extension` returns, in `.Some`, the text after the last `.` of the file name, without the `.`.
12. r[std-path.extension.none] It returns `.None` when there is no file name, when the name has no `.`, or when its only `.` is its first character.
13. r[std-path.extension.empty] A file name that ends in `.` gives `.Some("")`.

| Text | `parent` | `file_name` | `extension` |
| --- | --- | --- | --- |
| `"/a/b.txt"` | `.Some(Path("/a"))` | `.Some("b.txt")` | `.Some("txt")` |
| `"a/b/"` | `.Some(Path("a"))` | `.Some("b")` | `.None` |
| `"a"` | `.Some(Path(""))` | `.Some("a")` | `.None` |
| `"/a"` | `.Some(Path("/"))` | `.Some("a")` | `.None` |
| `"/"`, `""` | `.None` | `.None` | `.None` |
| `".bashrc"` | `.Some(Path(""))` | `.Some(".bashrc")` | `.None` |
| `"a.tar.gz"` | `.Some(Path(""))` | `.Some("a.tar.gz")` | `.Some("gz")` |
| `"a."` | `.Some(Path(""))` | `.Some("a.")` | `.Some("")` |
| `"a/.."` | `.Some(Path("a"))` | `.None` | `.None` |
| `"./a"` | `.Some(Path("."))` | `.Some("a")` | `.None` |

| `join` of | Result |
| --- | --- |
| `Path("a")` and `"b"` | `Path("a/b")` |
| `Path("a/")` and `"b"` | `Path("a/b")` |
| `Path("a")` and `"/b"` | `Path("/b")` |
| `Path("")` and `"b"` | `Path("b")` |

> **Why.** The methods and their edge cases are Rust's `Path::join`,
> `parent`, `file_name`, and `extension` on Unix, so the dotfile,
> trailing-slash, and root rows match there. Go's `filepath.Join` also
> cleans the result, which Rust and this chapter do not. `part` is a
> `string`, since hd has no `AsRef`; a `Path` argument is written
> `string(path)`.
