# `std.fs`

Generated from `hd doc std.fs`. Run `hd doc std.fs.NAME` in a package for one item's full text.

## `FsError`

```text
pub enum FsError
```

Every file system operation reports one of these errors: `NotFound`, `PermissionDenied`, `AlreadyExists`, `NotADirectory`, `IsADirectory`, `InvalidUtf8`, or `Other`.

## `EntryKind`

```text
pub enum EntryKind
```

Whether a file system entry is a file, a directory, or a symlink.

## `Entry`

```text
pub struct Entry
```

The value that describes one file system entry.

- `pub path: Path`: The entry's path.
- `pub kind: EntryKind`: The entry's kind.
- `pub size: u64`: The file's length in bytes.
- `pub modified: Timestamp? = .None`: The modification time, when the provider can report it.

## `FsRead`

```text
pub trait FsRead
```

The host capability to read from the file system: `read_bytes!`, `read_text!`, `list_dir!`, and `stat!`.

## `FsWrite`

```text
pub trait FsWrite
```

The host capability to change the file system: `write_bytes!`, `write_text!`, `append_text!`, `create_dir_all!`, `remove!`, `rename!`, and `copy!`.

## `read_text`

```text
pub fn read_text!(path: Path) -> Result[string, FsError] $ FsRead
```

Returns the whole content of the file at `path` as a string.

## `write_text`

```text
pub fn write_text!(path: Path, text: string) -> Result[void, FsError] $ FsWrite
```

Creates the file at `path`, or replaces its whole content.

## `copy`

```text
pub fn copy!(from: Path, to: Path) -> Result[void, FsError] $ FsWrite
```

Copies a file through the covering provider without reading its bytes into the program.

## `read_bytes`

```text
pub fn read_bytes!(path: Path) -> Result[List[u8], FsError] $ FsRead
```

Returns the whole content of the file at `path` as bytes.

## `list_dir`

```text
pub fn list_dir!(path: Path) -> Result[List[Entry], FsError] $ FsRead
```

Returns one `Entry` for each entry of the directory at `path`.

## `stat`

```text
pub fn stat!(path: Path) -> Result[Entry?, FsError] $ FsRead
```

Returns the `Entry` for `path`, or `.None` when nothing is there.

## `write_bytes`

```text
pub fn write_bytes!(path: Path, bytes: List[u8]) -> Result[void, FsError] $ FsWrite
```

Creates the file at `path` from `bytes`, or replaces its whole content.

## `append_text`

```text
pub fn append_text!(path: Path, text: string) -> Result[void, FsError] $ FsWrite
```

Adds `text` at the end of the file at `path`, creating the file when there is none.

## `create_dir_all`

```text
pub fn create_dir_all!(path: Path) -> Result[void, FsError] $ FsWrite
```

Creates the directory at `path` and every missing directory above it.

## `remove`

```text
pub fn remove!(path: Path) -> Result[void, FsError] $ FsWrite
```

Removes the file or the empty directory at `path`.

## `rename`

```text
pub fn rename!(from: Path, to: Path) -> Result[void, FsError] $ FsWrite
```

Moves the entry at `from` to `to`.

## `walk`

```text
pub fn walk!(root: Path) -> Result[List[Entry], FsError] $ FsRead
```

Returns every entry below `root`, depth first, each directory's entries ordered by path text and a directory before its contents.

## `glob`

```text
pub fn glob!(pattern: string) -> Result[List[Path], FsError] $ FsRead
```

Returns the paths that match `pattern`, ordered by path text, with `*`, `?`, `[...]` inside a component and `**` for any number of directories.

## `MemoryFs`

```text
pub struct MemoryFs
```

The deterministic `FsRead` and `FsWrite` provider: a file system in memory for unit tests.

- `pub fn new() -> *mut MemoryFs`: Returns an empty in-memory file system, as a box the providers and the test share.
- `pub fn set_modified(&mut self, path: Path, time: Timestamp) -> Result[void, FsError]`: Scripts an existing entry's modification time without a clock.
