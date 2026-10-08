# Working With Data

This page covers five everyday jobs that the
[Language Tour](LANGUAGE_TOUR.md) does not: reading JSON into typed
config, tests that live in the documentation, concurrent work with a
timeout, file reads and writes, and pulling fields out of log lines. Each
program below runs as written; the tests pass with `hd test FILE` (or in
package mode, `hd test`).

## Reading Configuration As JSON

A service's config file is the first thing that breaks in production, and
the first thing a reviewer reads. hd reads it as typed data: the fields
are typed, a missing one is a named error with its path, and an extra one
is ignored, so a newer config still loads with an older binary. The type
declares Serialize and Deserialize with `@derive(Serialize, Deserialize)`; nothing else
is needed. The exact rules are in
[Typed JSON](../spec/std/json.md#typed-json) and
[Serde](../spec/std/serde.md):

```hd
use std.json.{decode, JsonError}
use std.serde.{Deserialize, Serialize}
use std.testing.assert_equal

@derive(Serialize, Deserialize)
data Limits:
    retries: i32
    timeout_ms: i32

@derive(Serialize, Deserialize)
data Config:
    name: string
    limits: Limits

fn load(text: string) -> Result[Config, JsonError]:
    decode::[Config](text)

tests:
    it("reads a nested config"):
        text := "{\"name\": \"shop\", \"limits\": {\"retries\": 3, \"timeout_ms\": 250}}"
        match load(text):
            .Ok(config) =>
                assert_equal(config.name, "shop", reason="the name")
                assert_equal(config.limits.timeout_ms, 250, reason="the nested field")
            .Err(error) => panic("parse failed: $error")

    it("a missing key names its path"):
        match load("{\"name\": \"shop\", \"limits\": {\"retries\": 3}}"):
            .Ok(_) => panic("expected an error")
            .Err(error) => assert_equal("$error", "missing field at $.limits.timeout_ms", reason="the path")
```

A key that is present wins over the declared default; a key that is
missing with no default is an error. For the compact text of a value,
`encode` writes it; `decode` reads it back. Declare a count you will
compare with `len()` as `usize`, the type `len()` returns: a signed
field such as `i32` does not mix with a `usize` without a cast.

When the caller itself returns a `Result` of the same error, `?` passes
the error up, so no `match` is needed:

```hd
fn retry_limit(text: string) -> Result[i32, JsonError]:
    config := decode::[Config](text)?
    .Ok(config.limits.retries)
```

The function's error type must accept the error: either the same type,
or a type with a `From` implementation for it. So one function can use `?`
on a time parse and a number parse if its error enum marks a variant
`@from` for each ([Error Derivation](../spec/lang/14-annotations.md#error-derivation),
[`?` on a Result](../spec/lang/04-type-system.md#r-types.result.propagate.one-step)).

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
under `pkg` or from `std`. Run it with `hd test` in the package. The
file's own `use` lines go above the doc comment: the comment attaches to
the declaration immediately after it, so it cannot stand on a `use`
line, and no blank line may separate it from its declaration.

## Concurrent Work With A Timeout

A dashboard needs three lookups, and a user will not wait forever. `all!`
runs several suspending lookups together and returns their results in
order; `race!` returns the first to finish and cancels the rest. Both take
cold suspensions — plain calls, not bang calls — so `race!(fetch_user(),
slow())` starts two candidates and keeps one. A test binds a fake store
and a manual clock, so the timeout never actually fires. The exact rules
are in [Tasks](../spec/std/task.md). Two bounds of the pattern are worth
knowing before you extend it. An implementation's method must repeat the
trait's signature, row included, so a fake store that itself suspends —
a slow one — needs the trait to declare the row, as in
`fn lookup!(self, key: string) -> string $ Clock`. And a `ManualClock`'s
`sleep!` never waits: it completes at once and advances the virtual
time, so a test can check the fast path and the clock's reads, but not
which of two sleeps finishes first.

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

## Testing The Whole Program

A unit test runs on fakes; an integration test runs the real thing. Three
tools cover the rest of the job: `snapshot_file` compares rendered text
with a recorded file, `hd_run!` runs the package's own executable, and
`temp_dir` gives each test case a directory of its own that the runner
removes. The exact rules are in
[Snapshot Files](../spec/std/testing.md#snapshot-files) and
[Running Executables](../spec/std/testing.md#running-executables). In the
`shop` package, `src/report.hd` renders a report and
`tests/report.hd` checks it end to end:

```hd
use std.time.Timestamp

pub fn render(name: string, total: i32, at: Timestamp) -> string:
    "report $name: $total cents, at ${at.to_rfc3339()}"
```

```hd
use std.testing.{assert_equal, hd_run, snapshot_file, temp_dir}
use std.time.Timestamp
use pkg.report.render

it("renders the quarterly report"):
    snapshot_file(render("q4", 4500, Timestamp::from_unix_milliseconds(0)))

it("runs the built program in its own directory"):
    let out = hd_run!("shop")
    assert_equal(out.status, 0, reason="a clean exit")
    dir := temp_dir()
    assert_equal("$dir".len() > 0, true, reason="each case gets its own directory")
```

The first run fails until you record the snapshot with `hd test
--update`; every later run compares against it. A snapshot that should
change is re-recorded with another `--update`, and the diff is the review.
Tests that need deterministic randomness or arguments bind `SeededRandom`
or `MapArgs` the same way as `MemoryFs`.

## Publishing A Package

A package others fetch needs a version and a hash everyone can check. A
version is a git tag: `v2.3.0` at the repository root, `lint/v2.3.0` for
a package in the `lint` subdirectory. A manifest never states its own
version. `hd.sum`, committed beside the manifest, records the hash of
each selected tree, and a fetched tree that differs is rejected. The
exact rules are in [Versions](../spec/lang/10-modules.md#versions) and
[Integrity](../spec/lang/10-modules.md#integrity):

```sh
git tag v2.3.0        # version 2.3.0 at the repository root
git tag lint/v2.3.0   # version 2.3.0 of the package in lint/
```

A requirement is a minimum, and the build selects the largest minimum
named anywhere in the graph — minimal version selection, with no lockfile:

```toml
[dependencies]
json = "github.com/acme/json@2.1.0"   # a minimum: 2.3.0 may be selected
```

A manifest can also state the minimum toolchain that builds it, and a
root manifest can pin one exactly:

```toml
[package]
name = "shop"
hd = "0.2.0"   # the minimum toolchain that builds this package
```

`hd fetch` fetches every selected version the cache lacks, as CI does
before a build. Read next: [Dependencies](../spec/cli/command-line.md#dependencies).

## Deeper Collections

`List` and `Map` cover most days. Three more shapes cover the rest: `Set`
for distinct values, `sorted` and `group_by` for ordering and bucketing,
and the adapters on any iterator — `filter`, `map`, `fold`. The exact
rules are in [Collections](../spec/std/collections.md) and
[Iterators](../spec/std/iter.md):

```hd
use std.collections.Set
use std.testing.{assert_equal, it}

data Item:
    name: string
    price: i32
    tags: List[string]

fn sample() -> List[Item]:
    [Item { name: "apple", price: 3, tags: ["fruit", "fresh"] },
     Item { name: "bread", price: 5, tags: ["bakery"] },
     Item { name: "cheese", price: 8, tags: ["dairy", "fresh"] }]

tests:
    it("groups by tag and sorts by price"):
        items := sample()
        by_tag := items.group_by(fn(item: Item) -> string: item.tags[0])
        assert_equal(by_tag.get("fruit").map(fn(list): list.len()), .Some(1), reason="one fruit")
        cheap := items.sorted_by_key(fn(item: Item) -> i32: item.price)
        assert_equal(cheap[0].name, "apple", reason="cheapest first")

    it("collects each tag once"):
        let seen: mut Set[string] = Set::new()
        let kept: mut List[string] = []
        for item in sample():
            for tag in item.tags:
                if seen.insert(tag):
                    kept.push(tag)
        assert_equal(kept, ["fruit", "fresh", "bakery", "dairy"], reason="first occurrences, in item order")
```

A `Set` keeps each value once, in the order it first appeared. `Deque`
and `Heap` serve a queue and a priority queue; see
[Collections](../spec/std/collections.md).

## Points And Intervals In Time

Two types cover the clock: a `Timestamp` is a point in UTC time, and a
`Duration` is a distance between two points. Text in and out is RFC 3339,
so a config or a log line reads the same everywhere. An `Instant` is a
monotonic reading for measuring intervals, never a wall time. The exact
rules are in [Time](../spec/std/time.md):

```hd
use std.testing.{assert_equal, it}
use std.time.{Duration, Instant, Timestamp, min, s}

fn deadline(now: Timestamp) -> Timestamp:
    now + 30s

fn elapsed(start: Instant, end: Instant) -> Duration:
    end.since(start)

tests:
    it("reads and writes RFC 3339 text"):
        at := Timestamp::parse_rfc3339("2026-10-06T12:00:00Z").expect("valid")
        assert_equal(at.to_rfc3339(), "2026-10-06T12:00:00Z", reason="round trip")
        assert_equal(deadline(at).to_rfc3339(), "2026-10-06T12:00:30Z", reason="30s later")

    it("measures an interval"):
        let span: Duration = elapsed(Instant::from_milliseconds(1000), Instant::from_milliseconds(1600))
        assert_equal("$span", "600ms", reason="600 ms")
```

A `Duration` displays as `1h2min3.5s`, and `sleep!(d)` waits on the
clock. A test binds `ManualClock`, so waiting costs no real time; see
[Concurrent Work With A Timeout](#concurrent-work-with-a-timeout).

## Building Text

`trim` and `split` take text apart. The other direction has three tools:
`join` for a list with a separator, `StringBuilder` for a loop that
appends, and `repeat` for padding. `chars` and `bytes` walk a string by
character or by byte. The exact rules are in [Text](../spec/std/text.md):

```hd
use std.testing.{assert_equal, it}
use std.text.StringBuilder

fn csv_row(cells: List[string]) -> string:
    cells.join(",")

fn banner(text: string) -> string:
    let mut out = StringBuilder::new()
    out.push("=".repeat(4))
    out.push(" ")
    out.push(text)
    out.push(" ")
    out.push("=".repeat(4))
    out.build()

tests:
    it("joins and builds"):
        assert_equal(csv_row(["a", "b", "c"]), "a,b,c", reason="joined")
        assert_equal(banner("SALE"), "==== SALE ====", reason="built")

    it("reads characters and bytes"):
        assert_equal("héllo".chars().collect().len(), 5, reason="five chars")
        assert_equal("héllo".bytes().collect().len(), 6, reason="six bytes")
```

A string is bytes: `len()` counts bytes, `chars()` counts characters.
ASCII text needs no care; anything else, walk it with `chars()`.
