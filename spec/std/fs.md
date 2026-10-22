# Fs

Status: standard library specification draft.

This chapter defines `std.fs`, which `lib/std` writes in ordinary hd
over the language tier:

- the host capability traits `FsRead` and `FsWrite`;
- the error enum `FsError`, and the `Entry` value that describes a file;
- a free helper for each method of `FsRead` and `FsWrite`;
- `walk!` and `glob!`, which find entries below a directory;
- `MemoryFs`, the deterministic provider of both traits.

Paths are [`Path`](path.md) values. Which provider a command binds is CLI
tier ([Host Capabilities](../cli/command-line.md#host-capabilities)).

## File System Errors

Every file system operation reports one error enum:

```text
pub enum FsError:
    NotFound(path: Path)
    PermissionDenied(path: Path)
    NotGranted(path: Path)
    AlreadyExists(path: Path)
    NotADirectory(path: Path)
    IsADirectory(path: Path)
    InvalidUtf8(path: Path)
    Other(message: string)

pub enum EntryKind:
    File
    Directory
    Symlink

pub struct Entry:
    pub path: Path
    pub kind: EntryKind
    pub size: u64
    pub modified: Timestamp? = .None
```

1. r[std-fs.types.entry-metadata] `std.fs` declares `FsError`, `EntryKind`, and `Entry` as above. `Entry` is a struct value: a copy is independent of the entry it was copied from. Code imports them, as in `use std.fs.{Entry, FsError}`.
2. r[std-fs.error.one-enum] Every method of `FsRead` and `FsWrite` reports its failure as an `FsError`.
3. r[std-fs.error.other] `Other` holds a failure that no other variant names, with the host's message, or the message of a std call such as an invalid [glob](#walking-and-globbing) pattern.
4. r[std-fs.error.traits] `FsError`, `EntryKind`, and `Entry` implement `Eq`, and `FsError` implements `Display`.
5. r[std-fs.entry.size] An `Entry`'s `size` is the file's length in bytes.
6. r[std-fs.entry.modified] `Entry.modified` is the entry's modification time as a `Timestamp?`, defaulting to `.None`.
7. r[std-fs.entry.modified-host] Host providers fill `modified` from filesystem metadata when its time is available and representable as a `Timestamp`.
8. r[std-fs.entry.modified-unknown] A provider that cannot report the modification time returns `.None` for that field.
9. r[std-fs.error.not-granted] `NotGranted(path)` reports that the program's capability grant does not cover `path`, by [Partial Deny](../cli/command-line.md#partial-deny).
10. r[std-fs.error.permission-denied] `PermissionDenied(path)` reports that the operating system refused the access.
11. r[std-fs.error.not-granted.plain] The `Display` text of `NotGranted(path)` is `access to PATH is not granted`, with the path for `PATH`.
12. r[std-fs.error.eq.variant-payload] Two `FsError` values compare equal only when their variants and corresponding payloads are equal.

> **Why.** `NotGranted` and `PermissionDenied` have different fixes: a
> flag or an `hd.toml` entry for the first, file modes for the second.

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
    fn write_bytes!(&mut self, path: Path, bytes: List[u8]) -> Result[void, FsError]
    fn write_text!(&mut self, path: Path, text: string) -> Result[void, FsError]
    fn append_text!(&mut self, path: Path, text: string) -> Result[void, FsError]
    fn create_dir_all!(&mut self, path: Path) -> Result[void, FsError]
    fn remove!(&mut self, path: Path) -> Result[void, FsError]
    fn rename!(&mut self, from: Path, to: Path) -> Result[void, FsError]
    fn copy!(&mut self, from: Path, to: Path) -> Result[void, FsError]
```

1. r[std-fs.write.copy-decl] `std.fs` declares the host capability trait `FsWrite` with the methods above. Code imports it, as in `use std.fs.FsWrite`.
2. r[std-fs.write.replace] `write_bytes!` and `write_text!` create the file at `path`, or replace its whole content.
3. r[std-fs.write.append] `append_text!` adds `text` at the end of the file at `path`, and creates the file when there is none.
4. r[std-fs.write.create-dir-all] `create_dir_all!` creates the directory at `path` and every missing directory above it.
5. r[std-fs.write.remove] `remove!` removes the file or the empty directory at `path`.
6. r[std-fs.write.rename] `rename!` moves the entry at `from` to `to`.
7. r[std-fs.write.mut-ref.reference-values] Each `FsWrite` method takes `&mut self`, so `FsWrite` is a mutable requirement trait and `$.use(FsWrite)` is a `&mut dyn FsWrite`, by [`req.ref.use.reference-values`](../lang/11-requirements-and-suspension.md#r-req.ref.use.reference-values). Each `FsRead` method takes `self`, so `FsRead` is a readonly one, and a reference installed as its provider reads the place it refers to, by [`req.ref.share.reference-values`](../lang/11-requirements-and-suspension.md#r-req.ref.share.reference-values).
8. r[std-fs.cancel.write] Cancelling the suspension of `write_bytes!`, `write_text!`, or `append_text!` may leave a partial file at `path`.

## Copying Files

1. r[std-fs.copy.bytes] `FsWrite.copy!(from: Path, to: Path) -> Result[void, FsError]` copies the source file's bytes to the destination.
2. r[std-fs.copy.replace] Copying creates a missing destination file or replaces its entire content.
3. r[std-fs.copy.host] A host provider copies inside the host, without transferring the file's bytes into the program.
4. r[std-fs.copy.read-grant] The host checks the source against the `FsRead` grant, as for `read_bytes!`.
5. r[std-fs.copy.write-grant] The host checks the destination against the `FsWrite` grant, as for `write_bytes!`.
6. r[std-fs.copy.refused] A refused endpoint returns `NotGranted` naming that endpoint, before copying any bytes.
7. r[std-fs.copy.source-missing] A missing source returns `NotFound(from)`.
8. r[std-fs.copy.source-directory] A directory source returns `IsADirectory(from)`.
9. r[std-fs.copy.destination-directory] A destination directory returns `IsADirectory(to)`.
10. r[std-fs.copy.destination-parent-file] A destination below a file returns `NotADirectory(to)`.
11. r[std-fs.copy.alias] Copying a file to another path for the same file succeeds without changing its bytes.
12. r[std-fs.memory.copy] `MemoryFs.copy!` reads the source bytes and writes them to the destination with its existing read and write errors.
13. r[std-fs.helper.copy] `pub fn copy!(from: Path, to: Path) -> Result[void, FsError] $ FsWrite` calls `copy!(from, to)` on the covering `FsWrite` provider.

> **Why.** Copying is one host operation: a program need not read bytes
> only to send them straight back. Separate source and destination grants
> preserve the authority of the corresponding read and write operations.

## Suspension

1. r[std-fs.suspends] File system access is I/O, so every method of `FsRead` and `FsWrite` is a bang method.
2. r[std-fs.special-file] A read of a special file, such as a FIFO, a device, or a named pipe, lets other tasks run while it waits, by [`std-host.op.wait`](host.md#r-std-host.op.wait).
3. r[std-fs.special-file.deadline] Such a read has no other deadline guarantee.

```text
use std.fs.FsRead
use std.path.Path

fn show!(path: Path) -> string $ FsRead:
    match $.use(FsRead).read_text!(path):   # a bang call: it suspends on I/O
        .Ok(text) => text
        .Err(_) => ""
```

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
3. r[std-fs.helper.suspends] Every helper in this section is a bang function. `std` declares no helper that reads or writes a file without suspending.
4. r[std-fs.helper.read-bytes] `std.fs` declares `pub fn read_bytes!(path: Path) -> Result[List[u8], FsError] $ FsRead`, which calls `read_bytes!(path)` on the covering `FsRead` provider.
5. r[std-fs.helper.list-dir] `std.fs` declares `pub fn list_dir!(path: Path) -> Result[List[Entry], FsError] $ FsRead`, which calls `list_dir!(path)` on the covering `FsRead` provider.
6. r[std-fs.helper.stat] `std.fs` declares `pub fn stat!(path: Path) -> Result[Entry?, FsError] $ FsRead`, which calls `stat!(path)` on the covering `FsRead` provider.
7. r[std-fs.helper.write-bytes] `std.fs` declares `pub fn write_bytes!(path: Path, bytes: List[u8]) -> Result[void, FsError] $ FsWrite`, which calls `write_bytes!(path, bytes)` on the covering `FsWrite` provider.
8. r[std-fs.helper.append-text] `std.fs` declares `pub fn append_text!(path: Path, text: string) -> Result[void, FsError] $ FsWrite`, which calls `append_text!(path, text)` on the covering `FsWrite` provider.
9. r[std-fs.helper.create-dir-all] `std.fs` declares `pub fn create_dir_all!(path: Path) -> Result[void, FsError] $ FsWrite`, which calls `create_dir_all!(path)` on the covering `FsWrite` provider.
10. r[std-fs.helper.remove] `std.fs` declares `pub fn remove!(path: Path) -> Result[void, FsError] $ FsWrite`, which calls `remove!(path)` on the covering `FsWrite` provider.
11. r[std-fs.helper.rename] `std.fs` declares `pub fn rename!(from: Path, to: Path) -> Result[void, FsError] $ FsWrite`, which calls `rename!(from, to)` on the covering `FsWrite` provider.
12. r[std-fs.helper.row] A helper's row names exactly the capability of the method it calls: `FsRead` for `read_text!`, `read_bytes!`, `list_dir!`, and `stat!`, and `FsWrite` for the others.

```text
use std.fs.{FsError, append_text, list_dir, stat}
use std.fs.{EntryKind, FsRead, FsWrite}
use std.path.Path

fn log_line!(path: Path, line: string) -> Result[bool, FsError] $ FsRead + FsWrite:
    append_text!(path, "${line}\n")?
    .Ok(stat!(path)?.is_some())

fn count_files!(dir: Path) -> Result[usize, FsError] $ FsRead:
    let mut count: usize = 0
    for entry in list_dir!(dir)?:
        if entry.kind == EntryKind.File:
            count = count + 1
    .Ok(count)
```

> **Why.** A non-suspending wrapper would give each file operation two
> spellings. A program that reads files declares `main!`.
>
> **No `exists!`.** `stat!` already answers a missing path with `.None`, so
> `stat!(path)?.is_some()` says whether anything is there, and keeps a failed
> check, such as a denied path, apart from "missing". One atom, not two.

## Walking And Globbing

```text
use std.fs.{EntryKind, FsError, FsRead, glob, walk}
use std.path.Path

fn markdown!(root: Path) -> Result[List[Path], FsError] $ FsRead:
    let mut found: List[Path] = []
    for entry in walk!(root)?:
        if entry.kind == EntryKind.File && entry.path.extension() == .Some("md"):
            found.push(entry.path)
    .Ok(found)

fn markdown_again!() -> Result[List[Path], FsError] $ FsRead:
    glob!("docs/**/*.md")
```

`walk!` lists a whole tree and `glob!` lists the paths that fit a
pattern. Both read only through `list_dir!` and `stat!` of the covering
`FsRead` provider.

1. r[std-fs.walk.decl] `std.fs` declares `pub fn walk!(root: Path) -> Result[List[Entry], FsError] $ FsRead`.
2. r[std-fs.walk.entries] `walk!` returns one `Entry` for every entry below `root`, at any depth, and none for `root` itself. Each `Entry` is the one `list_dir!` returns for it.
3. r[std-fs.walk.order] The order is depth first: the entries of one directory come in the byte order of their path text, and a directory's entry comes directly before the entries under it.
4. r[std-fs.walk.symlinks] A symlink is one entry, with the kind `Symlink`. `walk!` does not look into it.
5. r[std-fs.walk.empty] A directory with no entries adds only its own entry, and an empty `root` gives an empty list.
6. r[std-fs.walk.error] When a `list_dir!` call fails, `walk!` returns that call's `.Err` and no list. So a missing `root` is `NotFound(root)`, and a file as `root` is `NotADirectory(root)` on a provider that reports it.
7. r[std-fs.glob.decl] `std.fs` declares `pub fn glob!(pattern: string) -> Result[List[Path], FsError] $ FsRead`.
8. r[std-fs.glob.components] `glob!` splits `pattern` at `/` into components and ignores empty ones. A pattern with no component matches nothing.
9. r[std-fs.glob.star] Within a component, `*` matches any run of characters, the empty run included.
10. r[std-fs.glob.one] `?` matches exactly one character.
11. r[std-fs.glob.class] `[` starts a class that ends at the next `]` and matches one character: one of the listed characters, or one in a range `low-high`. A `!` right after `[` makes the class match any character it does not list. A `-` is a range only between two characters, and is a plain character first or last in the class.
12. r[std-fs.glob.literal] Any other character matches itself, and there is no escape. To match `*`, `?`, or `[` itself, write it in a class, as in `[*]`.
13. r[std-fs.glob.dot] `*`, `?`, and a class match a leading `.` of a name like any other character.
14. r[std-fs.glob.deep] A component that is exactly `**` matches zero or more directories. The pieces `**` inside a longer component are two `*`.
15. r[std-fs.glob.deep-last] A `**` as the last component matches every entry below the directory before it, at any depth, and not that directory itself.
16. r[std-fs.glob.directories] A component before the last matches only a directory. The last component matches a file, a directory, or a symlink.
17. r[std-fs.glob.symlinks] No component, `**` included, looks into a symlink.
18. r[std-fs.glob.relative] A pattern that does not start with `/` is relative: `glob!` matches below the current directory, and its results do not start with `/`.
19. r[std-fs.glob.absolute] A pattern that starts with `/` is absolute: `glob!` matches below `/`, and its results start with `/`.
20. r[std-fs.glob.result] Each result is the matched components and entry names joined by `/`. The list is in the byte order of the path text, with no path twice.
21. r[std-fs.glob.none] A pattern that matches nothing gives `.Ok([])`. A directory that is missing or is a file matches nothing, and is not an error.
22. r[std-fs.glob.error] A pattern with an unclosed `[`, an empty class (`[]` or `[!]`), or a range whose low end is after its high end is `.Err(FsError.Other(message))`, where `message` is `invalid glob pattern PATTERN: REASON`. `REASON` is `unclosed [`, `empty class`, or `range LOW-HIGH runs backwards`.
23. r[std-fs.glob.error.first] `glob!` checks every component before it reads the file system, so a bad pattern fails even when an earlier component matches nothing. A `list_dir!` or `stat!` failure ends `glob!` with that call's `.Err`.
24. r[std-fs.glob.row] `walk!` and `glob!` have the row `FsRead`, and they are bang functions by [`std-fs.helper.suspends`](#r-std-fs.helper.suspends).

> **Why.** The tree walk is the loop every file tool writes by hand, and
> a pattern is how people name a set of files. Both build on `FsRead`, so
> `MemoryFs` tests them with no host support. Go's `filepath.WalkDir`
> and Rust's `walkdir` also visit a directory before its contents. A fixed
> order makes the result the same on every host.
>
> **Why symlinks stay closed.** A symlink to a parent directory would make
> a walk run forever. `walkdir` follows links only on request,
> and Go's `WalkDir` never follows them. A program that wants links followed reads
> the `Symlink` entry and walks its target.
>
> **Why `glob!` is relative to the current directory.** Python's `glob`
> and the `glob` crate resolve a relative pattern the same way, and `!`
> negates a class in both.

## Memory File System

`MemoryFs` is the deterministic provider of `FsRead` and `FsWrite`. It
holds its files in memory, and a test supplies it with `$.with`:

```text
use std.fs.{FsError, FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.path.Path
fn copy_notes!() -> Result[void, FsError] $ FsRead + FsWrite:
    text := read_text!(Path("notes.txt"))?
    write_text!(Path("notes.bak"), text)

tests:
    use std.testing.assert

    it("copies in memory"):
        files := MemoryFs::new()
        assert(files.write_text!(Path("notes.txt"), "tea").is_ok(), reason="seeded")
        $.with(FsRead=files, FsWrite=files):
            assert(copy_notes!().is_ok(), reason="copied")
        assert(files.read_text!(Path("notes.bak")).unwrap_or("") == "tea", reason="the copy")
```

1. r[std-fs.memory.decl] `std.fs` declares `MemoryFs`, which implements `FsRead` and `FsWrite`, with private fields. Code imports it, as in `use std.fs.MemoryFs`.
2. r[std-fs.memory.new-ref] `MemoryFs::new() -> &mut MemoryFs` returns a new object holding a file system with no files and no directories. `MemoryFs` is a struct used behind that reference: installing it as `FsRead=files, FsWrite=files` makes both providers refer to the one object, so a test reads back through `files` what the code under test wrote.
3. r[std-fs.memory.normalized-keys] Every `MemoryFs` operation drops empty and `.` components from its path keys, so `./a.txt` and `a.txt` name one entry.
4. r[std-fs.memory.rooted-keys] A normalized key keeps a leading `/` when the supplied path starts with `/`.
5. r[std-fs.memory.literal-parent] Normalization leaves `..` components unchanged, without resolving parent directories.
6. r[std-fs.memory.normalized-root] A relative path with no remaining components names `.`, and an absolute one names `/`.
7. r[std-fs.memory.supplied-errors] Errors retain the caller's supplied path, including its original spelling.
8. r[std-fs.memory.dirs] A directory exists when `create_dir_all!` made it, or when its path is the text before a `/` in the path of a file. So writing a file makes the directories above it.
9. r[std-fs.memory.create-dir-all] `create_dir_all!` records `path` and every directory above it, so each stays after the entries inside it are removed.
10. r[std-fs.memory.entry] A file's `Entry` has the kind `File` and its length in bytes. A directory's has the kind `Directory` and the size 0. No entry is a `Symlink`.
11. r[std-fs.memory.normalized-list-order] `list_dir!` returns the entries directly inside the directory, in the byte order of their path text. The returned entry paths use normalized keys.
12. r[std-fs.memory.normalized-root-directory] The roots `.` and `/` always exist, and `stat!` returns a `Directory` entry for either.
13. r[std-fs.memory.relative-root-list] `list_dir!(Path("."))` lists direct relative children with bare paths, such as `a.md`.
14. r[std-fs.memory.rename] `rename!` of a directory moves every entry under it as well.
15. r[std-fs.memory.no-host] A `MemoryFs` never reads or changes the host's file system.

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

## Scripted Modification Times

`MemoryFs` records only the times a test supplies; it has no clock.

1. r[std-fs.memory.modified-none] A `MemoryFs` entry has `modified` equal to `.None` until its time is scripted.
2. r[std-fs.memory.set-modified] `set_modified(&mut self, path: Path, time: Timestamp) -> Result[void, FsError]` sets an existing file's or directory's modification time.
3. r[std-fs.memory.modified-missing] Setting a time on a missing path returns `.Err(FsError.NotFound(path))` with the supplied path.
4. r[std-fs.memory.modified-write] A successful `write_bytes!`, `write_text!`, or `append_text!` clears that entry's scripted time to `.None`.
5. r[std-fs.memory.modified-rename] Renaming an entry moves its scripted time, including the times of descendants moved with a directory.
6. r[std-fs.memory.modified-remove] Removing an entry removes its scripted time.

> **Why.** A test can script deterministic ordering without reading a
> clock. After a write, retaining an old scripted time could hide stale
> metadata, so the new time is unknown until the test supplies it again.
