# Work With Files And Directories

**Scenario.** A cleanup tool walks a project folder, finds the large log
files, writes a report, and moves the old logs into an archive folder.

## The program

```hd
use std.fs.{Entry, EntryKind, FsError, FsRead, FsWrite, MemoryFs, create_dir_all, list_dir, read_text, rename, stat, walk, write_text}
use std.path.Path

struct Found:
    path: Path
    size: u64

fn is_log(entry: &Entry) -> bool:
    match entry.kind:
        .File => entry.path.extension() == .Some("log")
        _ => false

# Every file below `root`, at any depth.
fn large_logs!(root: Path, min_bytes: u64) -> Result[List[Found], FsError] $ FsRead:
    let mut found: List[Found] = []
    for entry in walk!(root)?:
        if is_log(&entry) && entry.size >= min_bytes:
            found.push(Found { path: entry.path, size: entry.size })
    .Ok(found.sorted_by_key(fn(item): "${item.path}"))

fn write_report!(path: Path, found: List[Found]) -> Result[void, FsError] $ FsWrite:
    lines := found.map(fn(item): "${item.size}\t${item.path}")
    write_text!(path, lines.join("\n") + "\n")

# Move each file into `archive/`, keeping its name.
fn archive!(found: List[Found], archive: Path) -> Result[usize, FsError] $ FsWrite:
    create_dir_all!(archive)?
    for item in found:
        name := item.path.file_name().unwrap_or("unnamed.log")
        rename!(item.path, archive.join(name))?
    .Ok(found.len())

pub fn main!() -> Result[void, FsError] $ Console + FsRead + FsWrite:
    found := large_logs!(Path("."), 1024)?
    write_report!(Path("large-logs.txt"), found)?
    moved := archive!(found, Path("archive"))?
    println("archived $moved files")
    .Ok(())

tests:
    it("finds, reports and archives the large logs"):
        fs := MemoryFs::new()
        $.with(FsWrite=fs, FsRead=fs):
            _ := create_dir_all!(Path("app/deep"))      # folders first, as on a real disk
            _ := write_text!(Path("app/run.log"), "x".repeat(40))
            _ := write_text!(Path("app/deep/old.log"), "x".repeat(50))
            _ := write_text!(Path("app/small.log"), "x")
            _ := write_text!(Path("app/notes.txt"), "x".repeat(99))

            found := large_logs!(Path("app"), 10).expect("a readable folder")
            assert_equal(found.map(fn(item): "${item.path}"), ["app/deep/old.log", "app/run.log"])

            assert(write_report!(Path("report.txt"), found).is_ok())
            assert_equal(read_text!(Path("report.txt")).ok(), .Some("50\tapp/deep/old.log\n40\tapp/run.log\n"))

            assert_equal(archive!(found, Path("archive")).ok(), .Some(2))
            assert_equal(stat!(Path("app/run.log")).ok(), .Some(.None))
            names := list_dir!(Path("archive")).expect("the archive exists").map(fn(entry): entry.path.file_name().unwrap_or(""))
            assert_equal(names.sorted(), ["old.log", "run.log"])
```

## The pieces

- **Paths** are `Path` values: `Path("logs/app.log")`. Build with
  `path.join("name")`; take apart with `parent()`, `file_name()`,
  `file_stem()` and `extension()`, which return optionals. `"$path"` is
  its text.
- **Listing**: `list_dir!(dir)` gives the entries of one folder,
  `walk!(root)` everything below it, and `glob!("src/**/*.hd")` the paths
  that match a pattern. An `Entry` has `path`, `kind` (`.File`,
  `.Directory`, `.Symlink`), `size` and `modified`.
- **Does it exist?** `stat!(path)` gives `.Ok(.None)` when nothing is
  there; reading a missing file is an `.Err`.
- **Reading**: `read_text!(path)` for text, `read_bytes!(path)` for
  `List[u8]`. A whole file is read at once; `text.lines()` splits it.
- **Writing**: `write_text!` replaces the file, and `append_text!` adds to
  its end. Neither creates a missing folder: the result is
  `.Err(.NotFound(...))`. Call `create_dir_all!(folder)` first, in the
  program and when seeding a `MemoryFs` in a test. The same holds for the
  destination of `copy!` and `rename!`.
- **Moving and deleting**: `rename!(from, to)`, `copy!(from, to)`,
  `remove!(path)`.
- **Capabilities**: `$ FsRead` and `$ FsWrite` are separate, so a function
  that only reads cannot write. Grant folders in `hd.toml`:
  `FsRead = ["."]`, `FsWrite = ["archive/", "large-logs.txt"]`
  ([capabilities](../design/capabilities.md)).
- **Testing**: one `MemoryFs::new()` installed for both traits. Nothing
  touches the disk. An integration test gets a real temporary folder from
  `temp_dir()` ([testing](../design/testing.md#integration-tests)).

For a fresh workspace, use `using! dir = fs.temp_dir!("work-")?` under
`$ FsWrite`; `dir.path` is a `Path`. Import the `std.fs` module, or import
`temp_dir` and call `temp_dir!` directly. The guard removes the tree on
normal, error, and cancelled exit, using its creating provider. Scoped grants
can use `FsWrite = ["temp"]`; reading the workspace also needs `FsRead`.
