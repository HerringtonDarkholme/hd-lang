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
## Scanning Logs With Regex

Operations read logs: count the server errors by status, or find the
minute that broke. `std.regex` compiles a pattern once and captures groups
by number or name; it runs in linear time, so a hostile log line cannot
stall the scan. The exact rules are in [Regex](../spec/std/regex.md):

```hd
use std.num.parse_i32
use std.regex.Regex
use std.text.r
use std.testing.{assert_equal, it}

fn status_of(line: string) -> i32?:
    match Regex::new(r"\"[A-Z]+ /[^ ]*\" (\d\d\d)"):
        .Ok(pattern) =>
            match pattern.captures(line):
                .Some(found) => found.get(1).map(fn(m): parse_i32(m.text).expect("digits"))
                .None => .None
        .Err(_) => .None

fn errors_by_status(lines: List[string]) -> Map[i32, usize]:
    let counts: mut Map[i32, usize] = {}
    for line in lines:
        match status_of(line):
            .Some(code) => if code >= 500:
                counts[code] = counts.get(code).unwrap_or(0) + 1
            .None => pass
    counts

tests:
    it("counts server errors by status"):
        lines := ["\"GET /ok\" 200", "\"POST /x\" 500", "\"GET /y\" 502", "\"GET /z\" 500"]
        assert_equal(errors_by_status(lines).get(500), .Some(2), reason="two 500s")
        assert_equal(errors_by_status(lines).get(502), .Some(1), reason="one 502")
```

Write the pattern as a raw string, `r"..."`, so `\d` needs no second
backslash. A pattern that does not compile is a `RegexError`, not a panic.
