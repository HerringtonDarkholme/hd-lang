# `std.path`

Generated from `hd doc std.path`. Run `hd doc std.path.NAME` in a package for one item's full text.

## `Path`

```text
pub type Path(string)
```

A file system path: text the path operations read by `/` components.

- `pub fn join(&self, part: string) -> Path`: The path with `part` joined on by `/`; an absolute `part` replaces the whole path.
- `pub fn parent(&self) -> Path?`: The path without its last component, in `.Some`, or `.None` when it has none.
- `pub fn file_name(&self) -> string?`: The path's last component, in `.Some`, or `.None` when it has none.
- `pub fn extension(&self) -> string?`: The text after the last dot of the file name, in `.Some`, or `.None`.
- `pub fn file_stem(&self) -> string?`: The file name without its extension, in `.Some`, or `.None` when there is no file name.
- `pub fn with_extension(&self, extension: string) -> Path`: The path with the extension of its file name replaced by `extension`, or removed when it is empty.
