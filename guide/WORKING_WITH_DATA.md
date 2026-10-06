# Working With Data

This page covers five everyday jobs that the
[Language Tour](LANGUAGE_TOUR.md) does not: reading JSON into typed
config, tests that live in the documentation, concurrent work with a
timeout, file reads and writes, and pulling fields out of log lines. Each
program below runs as written; the tests pass with `hd test FILE` (or in
package mode, `hd test`).
# Working With Data

This page covers five everyday jobs that the
[Language Tour](LANGUAGE_TOUR.md) does not: reading JSON into typed
config, tests that live in the documentation, concurrent work with a
timeout, file reads and writes, and pulling fields out of log lines. Each
program below runs as written; the tests pass with `hd test FILE` (or in
package mode, `hd test`).
## Documentation That Runs As Tests

A code block in a doc comment that no one runs drifts out of date. A
fenced `hd` block inside a `##` documentation comment is a doc test: `hd
test` runs it like any other test, so the example in the docs is the
example that compiles today. The rules are in
[Doc Tests](../spec/lang/10-modules.md#doc-tests):

```hd
## Money helpers.
##
## The example runs as a test:
## ```hd
## use pkg.total
## use std.testing.assert_equal
##
## assert_equal(total([+300, +200]), +500, reason="sum")
## ```
pub fn total(prices: List[i32]) -> i32:
    let sum = +0
    for price in prices:
        sum = sum + price
    sum
```

A doc test is its own program: it imports what it uses, from the package
under `pkg` or from `std`. Run it with `hd test` in the package.
## Concurrent Work With A Timeout

A dashboard needs three lookups, and a user will not wait forever. `all!`
runs several suspending lookups together and returns their results in
order; `race!` returns the first to finish and cancels the rest. Both take
cold suspensions — plain calls, not bang calls — so `race!(fetch_user(),
slow())` starts two candidates and keeps one. A test binds a fake store
and a manual clock, so the timeout never actually fires. The exact rules
are in [Tasks](../spec/std/task.md):

```hd
use std.task.{all, race}
use std.testing.{assert_equal, it}
use std.time.{Clock, ManualClock, Timestamp, s, sleep}

trait Store:
    fn lookup!(self, key: string) -> string

fn fetch_user!() -> string $ Store:
    $.use(Store).lookup!("user")

fn fetch_orders!() -> string $ Store:
    $.use(Store).lookup!("orders")

fn fetch_profile!() -> string $ Store:
    $.use(Store).lookup!("profile")

fn load_page!() -> (string, string, string) $ Store:
    all!(fetch_user(), fetch_orders(), fetch_profile())

fn slow!() -> string $ Clock:
    sleep!(s(60))
    "too slow"

fn load_with_timeout!() -> string $ Store + Clock:
    race!(fetch_user(), slow())

data FakeStore:
    tag: string

impl Store for FakeStore:
    fn lookup!(self, key: string) -> string:
        "fake-$key"

tests:
    it("loads all three at once"):
        $.with(Store=FakeStore { tag: "t" }):
            assert_equal(load_page!(), ("fake-user", "fake-orders", "fake-profile"), reason="all three")

    it("a fast lookup beats the timeout"):
        $.with(Store=FakeStore { tag: "t" }):
            $.with(Clock=ManualClock::new(Timestamp::from_unix_milliseconds(0))):
                assert_equal(load_with_timeout!(), "fake-user", reason="the lookup wins")
```
## Reading And Writing Files

Importing a CSV, writing a report: the file system is a capability, so a
test never touches a real one. `FsWrite` writes, `FsRead` reads, and a
unit test binds `MemoryFs` for both. The exact rules are in
[Fs](../spec/std/fs.md):

```hd
use std.fs.{FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.path.Path
use std.testing.{assert_equal, it}

fn save_report!(name: string, total: i32) -> void $ FsWrite:
    _ := write_text!(Path("reports/$name.txt"), "total: $total")

fn read_report!(name: string) -> string $ FsRead:
    read_text!(Path("reports/$name.txt")).expect("the report exists")

tests:
    it("writes and reads back a report"):
        let mut fs = MemoryFs::new()
        $.with(FsWrite=fs, FsRead=fs):
            save_report!("q4", 4500)
            assert_equal(read_report!("q4"), "total: 4500", reason="round trip")
```

Paths are `Path` values, so `"reports/$name.txt"` is one argument, not
string surgery on separators. Integration tests get the real file system
and a fresh temporary directory per case; see
[Temporary Directories](../spec/std/testing.md#temporary-directories).
