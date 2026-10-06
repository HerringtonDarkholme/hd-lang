# Fs

Status: standard library specification draft.

This chapter defines `std.fs`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability traits `FsRead` and `FsWrite`;
- the error enum `FsError`, and the `Entry` data that describes a file;
- the helpers `read_text!` and `write_text!`;
- `MemoryFs`, the deterministic provider of both traits.

Paths are [`Path`](path.md) values. Which provider a command binds is CLI
tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).

## File System Errors

Every file system operation reports one error enum:

```text
pub enum FsError:
    NotFound(path: Path)
    PermissionDenied(path: Path)
    AlreadyExists(path: Path)
    NotADirectory(path: Path)
    IsADirectory(path: Path)
    InvalidUtf8(path: Path)
    Other(message: string)

pub enum EntryKind:
    File
    Directory
    Symlink

pub data Entry:
    pub path: Path
    pub kind: EntryKind
    pub size: u64
```

1. r[std-fs.error.decl] `std.fs` declares `FsError`, `EntryKind`, and `Entry` as above. Code imports them, as in `use std.fs.{Entry, FsError}`.
2. r[std-fs.error.one-enum] Every method of `FsRead` and `FsWrite` reports its failure as an `FsError`.
3. r[std-fs.error.other] `Other` holds a failure that no other variant names, with the host's message.
4. r[std-fs.error.traits] `FsError`, `EntryKind`, and `Entry` implement `Eq`, and `FsError` implements `Display`.
5. r[std-fs.entry.size] An `Entry`'s `size` is the file's length in bytes.

## Reading

```text
pub trait FsRead:
    fn read_bytes!(self, path: Path) -> Result[List[u8], FsError]
    fn read_text!(self, path: Path) -> Result[string, FsError]
    fn list_dir!(self, path: Path) -> Result[List[Entry], FsError]
    fn stat!(self, path: Path) -> Result[Entry?, FsError]
```

1. r[std-fs.read.decl] `std.fs` declares the host capability trait `FsRead` with the methods above. Code imports it, as in `use std.fs.FsRead`.
2. r[std-fs.read.bytes] `read_bytes!` returns the whole content of the file at `path`.
3. r[std-fs.read.text] `read_text!` returns that content as a string. Content that is not valid UTF-8 is `.Err(FsError.InvalidUtf8(path))`.
4. r[std-fs.read.list-dir] `list_dir!` returns one `Entry` for each entry of the directory at `path`.
5. r[std-fs.read.stat] `stat!` returns the `Entry` for `path`, or `.None` when nothing is there.

## Writing

```text
pub trait FsWrite:
    fn write_bytes!(mut self, path: Path, bytes: List[u8]) -> Result[void, FsError]
    fn write_text!(mut self, path: Path, text: string) -> Result[void, FsError]
    fn append_text!(mut self, path: Path, text: string) -> Result[void, FsError]
    fn create_dir_all!(mut self, path: Path) -> Result[void, FsError]
    fn remove!(mut self, path: Path) -> Result[void, FsError]
    fn rename!(mut self, from: Path, to: Path) -> Result[void, FsError]
```

1. r[std-fs.write.decl] `std.fs` declares the host capability trait `FsWrite` with the methods above. Code imports it, as in `use std.fs.FsWrite`.
2. r[std-fs.write.replace] `write_bytes!` and `write_text!` create the file at `path`, or replace its whole content.
3. r[std-fs.write.append] `append_text!` adds `text` at the end of the file at `path`, and creates the file when there is none.
4. r[std-fs.write.create-dir-all] `create_dir_all!` creates the directory at `path` and every missing directory above it.
5. r[std-fs.write.remove] `remove!` removes the file or the empty directory at `path`.
6. r[std-fs.write.rename] `rename!` moves the entry at `from` to `to`.
7. r[std-fs.write.mut] Each `FsWrite` method takes `mut self`, so `FsWrite` is a mutable requirement trait, and `FsRead` is a readonly one.

## Suspension

1. r[std-fs.suspends] File system access is I/O, so every method of `FsRead` and `FsWrite` is a bang method.

> **Why.** Reading and writing are separate traits, so a row says which
> one a function needs. A program that only reads gains no authority
> to change files.

## File Helpers

```text
use std.fs.{FsError, FsRead, FsWrite, read_text, write_text}
use std.path.Path

fn copy_notes!() -> Result[void, FsError] $ FsRead + FsWrite:
    text := read_text!(Path("notes.txt"))?
    write_text!(Path("notes.bak"), text)
```

1. r[std-fs.helper.read-text] `std.fs` declares `pub fn read_text!(path: Path) -> Result[string, FsError] $ FsRead`, which calls `read_text!(path)` on the `FsRead` provider that covers the call.
2. r[std-fs.helper.write-text] `std.fs` declares `pub fn write_text!(path: Path, text: string) -> Result[void, FsError] $ FsWrite`, which calls `write_text!(path, text)` on the `FsWrite` provider that covers the call.
3. r[std-fs.helper.suspends] Both helpers are bang functions. `std` declares no helper that reads or writes a file without suspending.

> **Why.** A non-suspending wrapper would give each file operation two
> spellings. A program that reads files declares `main!`.

## Memory File System

`MemoryFs` is the deterministic provider of `FsRead` and `FsWrite`. It
holds its files in memory, and a test supplies it with `$.with`:

```text
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.path.Path
use std.testing.assert

fn copy_notes!() -> Result[void, FsError] $ FsRead + FsWrite:
    text := read_text!(Path("notes.txt"))?
    write_text!(Path("notes.bak"), text)

tests:
    it("copies in memory"):
        let mut files = MemoryFs::new()
        assert(files.write_text!(Path("notes.txt"), "tea").is_ok(), reason="seeded")
        $.with(FsRead=files, FsWrite=files):
            assert(copy_notes!().is_ok(), reason="copied")
        assert(files.read_text!(Path("notes.bak")).unwrap_or("") == "tea", reason="the copy")
```

1. r[std-fs.memory.decl] `std.fs` declares `MemoryFs`, which implements `FsRead` and `FsWrite`, with private fields. Code imports it, as in `use std.fs.MemoryFs`.
2. r[std-fs.memory.new] `MemoryFs::new() -> mut MemoryFs` returns a file system with no files and no directories.
3. r[std-fs.memory.keys] A `MemoryFs` names an entry by its path's text, as written, so `Path("a.txt")` and `Path("./a.txt")` are different files.
4. r[std-fs.memory.dirs] A directory exists when `create_dir_all!` made it, or when its path is the text before a `/` in the path of a file. So writing a file makes the directories above it.
5. r[std-fs.memory.create-dir-all] `create_dir_all!` records `path` and every directory above it, so each stays after the entries inside it are removed.
6. r[std-fs.memory.entry] A file's `Entry` has the kind `File` and its length in bytes. A directory's has the kind `Directory` and the size 0. No entry is a `Symlink`.
7. r[std-fs.memory.list-order] `list_dir!` returns the entries directly inside the directory, in the byte order of their path text. Each entry's path is the directory's path, `/`, and the entry's name.
8. r[std-fs.memory.rename] `rename!` of a directory moves every entry under it as well.
9. r[std-fs.memory.no-host] A `MemoryFs` never reads or changes the host's file system.

Its failures are these, and it reports no other. Each error holds the
path that fails, which for a rename is `to` unless `from` is missing:

| Rule | Operation | Error |
| --- | --- | --- |
| r[std-fs.memory.error.not-found] Nothing there | reading, listing, removing, or renaming a path where no entry is | `NotFound(path)`, with the missing path |
| r[std-fs.memory.error.is-directory] A directory as a file | reading a directory, or writing, appending, or renaming a file to a directory's path | `IsADirectory(path)` |
| r[std-fs.memory.error.not-directory] A file as a directory | listing a file, creating a directory at a file's path, renaming a directory onto a file, or writing, appending, creating, or renaming to a path below a file | `NotADirectory(path)` |
| r[std-fs.memory.error.not-empty] A directory with entries | removing it | `Other(message)` |
| r[std-fs.memory.error.utf8] Not text | `read_text!` of content that is not valid UTF-8 | `InvalidUtf8(path)`, as [`std-fs.read.text`](#r-std-fs.read.text) states |

> **Why.** A test states the files its code reads, and reads back what the
> code wrote, with nothing on disk to set up or clean. Go's `fstest.MapFS`
> serves tests the same way.

See also: [Path](path.md), [Mutable Providers](../lang/11-requirements-and-suspension.md#mutable-providers).
