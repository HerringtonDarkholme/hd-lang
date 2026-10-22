# Working With Data

This page covers everyday jobs that the [tour](../tour/01-hello.md) and
[Learn hd in 10 Minutes](LEARN_IN_10_MINUTES.md) do not: read-only access
to data, reading JSON into typed config, tests that live in the documentation, concurrent work
with a timeout, file reads and writes, pulling fields out of log lines,
wiring a program's providers in `main`, and splitting a program into
packages. Each `hd` program below runs as written; the tests pass with
`hd test FILE` (or in package mode, `hd test`).

## Read-only data

A `&T` to plain values, including structs, lists and maps, is read-only all
the way down while the path stays within those values. It prevents replacing
a field or changing a nested collection through that reference.

A contained `&mut` keeps its write access. In this team dashboard, `view`
cannot rename the team, but its `owner` reference can update Ada's completed
work. Copying that reference keeps the same access:

```hd
pub struct User:
    pub name: string
    pub completed: usize

impl User:
    pub fn complete_task(&mut self) -> void:
        self.completed += 1

pub struct Team:
    name: string
    owner: &mut User

impl Team:
    pub fn lead(&self) -> &User:
        self.owner

    pub fn lead_mut(&mut self) -> &mut User:
        self.owner

pub fn main() -> void $ Console:
    let mut ada = User { name: "Ada", completed: 0 }
    let mut team = Team { name: "Shipping", owner: &mut ada }
    view := &team
    view.owner.complete_task()
    copied := view.owner
    copied.complete_task()
    editor := &mut team
    editor.lead_mut().complete_task()
    reader := view.lead()
    println("${view.name}: ${reader.name}, ${reader.completed} jobs")
```

```output
Shipping: Ada, 3 jobs
```

To hand out read-only access to a user, declare the field `&User`. Another
option is a private `&mut User` field with the getter pair above:
`lead(&self) -> &User` gives read access, and
`lead_mut(&mut self) -> &mut User` gives write access. Outside this module,
a `&Team` holder can only read the user; writing requires a `&mut Team`
holder to call `lead_mut`. The direct field accesses in this example work
because they are inside the module that declares the private field.
A trait exposing `&self` getters can provide the same read-only interface.
The rules are in [Reading Through A Reference](../../spec/lang/13-values-and-references.md#reading-through-a-reference).

## Reading Configuration As JSON

A service's config file is the first thing that breaks in production, and
the first thing a reviewer reads. hd reads it as typed data: the fields
are typed, a missing one is a named error with its path, and an extra one
is ignored, so a newer config still loads with an older binary. The type
declares Serialize and Deserialize with `@derive(Serialize, Deserialize)`; nothing else
is needed. The exact rules are in
[Typed JSON](../../spec/std/json.md#typed-json) and
[Serde](../../spec/std/serde.md):

```hd
use std.json.{decode, JsonError}
use std.serde.{Deserialize, Serialize}
@derive(Serialize, Deserialize)
struct Limits:
    retries: i32
    timeout_ms: i32

@derive(Serialize, Deserialize)
struct Config:
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
use std.json.{decode, JsonError}
use std.serde.{Deserialize, Serialize}

@derive(Serialize, Deserialize)
struct Limits:
    retries: i32
    timeout_ms: i32

@derive(Serialize, Deserialize)
struct Config:
    name: string
    limits: Limits

fn retry_limit(text: string) -> Result[i32, JsonError]:
    config := decode::[Config](text)?
    .Ok(config.limits.retries)
```

The function's error type must accept the error: either the same type,
or a type with a `From` implementation for it. So one function can use `?`
on a time parse and a number parse if its error enum marks a variant
`@from` for each ([Error Derivation](../../spec/lang/14-annotations.md#error-derivation),
[`?` on a Result](../../spec/lang/04-type-system.md#r-types.result.propagate.one-step)).

## Documentation That Runs As Tests

A code block in a doc comment that no one runs drifts out of date. A
fenced `hd` block inside a `##` documentation comment is a doc test: `hd
test` runs it like any other test, so the example in the docs is the
example that compiles today. The rules are in
[Doc Tests](../../spec/lang/10-modules.md#doc-tests):

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
are in [Tasks](../../spec/std/task.md). Two bounds of the pattern are worth
knowing before you extend it. An implementation's method must repeat the
trait's signature, row included, so a fake store that itself suspends —
a slow one — needs the trait to declare the row, as in
`fn lookup!(&self, key: string) -> string $ Clock`. And a `ManualClock`'s
`sleep!` never waits: it completes at once and advances the virtual
time, so a test can check the fast path and the clock's reads, but not
which of two sleeps finishes first.

```hd
use std.task.{all, race}
use std.time.{Clock, ManualClock, Timestamp, s, sleep}

trait Store:
    fn lookup!(&self, key: string) -> string

fn fetch_user!() -> string $ Store:
    $.use(Store).lookup!("user")

fn fetch_orders!() -> string $ Store:
    $.use(Store).lookup!("orders")

fn fetch_profile!() -> string $ Store:
    $.use(Store).lookup!("profile")

fn load_page!() -> (string, string, string) $ Store:
    all!(fetch_user(), fetch_orders(), fetch_profile())

fn slow!() -> string $ Clock:
    sleep!(60s)
    "too slow"

fn load_with_timeout!() -> string $ Store + Clock:
    race!(fetch_user(), slow())

struct FakeStore:
    tag: string

impl Store for FakeStore:
    fn lookup!(&self, key: string) -> string:
        "fake-$key"

tests:
    it("loads all three at once"):
        $.with(Store=&FakeStore { tag: "t" }):
            assert_equal(load_page!(), ("fake-user", "fake-orders", "fake-profile"), reason="all three")

    it("a fast lookup beats the timeout"):
        $.with(Store=&FakeStore { tag: "t" }):
            $.with(Clock=ManualClock::new(Timestamp::from_unix_milliseconds(0))):
                assert_equal(load_with_timeout!(), "fake-user", reason="the lookup wins")
```

## Reading And Writing Files

Importing a CSV, writing a report: the file system is a capability, so a
test never touches a real one. `FsWrite` writes, `FsRead` reads, and a
unit test binds `MemoryFs` for both. The exact rules are in
[Fs](../../spec/std/fs.md):

```hd
use std.fs.{FsRead, FsWrite, MemoryFs, read_text, write_text}
use std.path.Path
fn save_report!(name: string, total: i32) -> void $ FsWrite:
    _ := write_text!(Path("reports/$name.txt"), "total: $total")

fn read_report!(name: string) -> string $ FsRead:
    read_text!(Path("reports/$name.txt")).expect("the report exists")

tests:
    it("writes and reads back a report"):
        fs := MemoryFs::new()
        $.with(FsWrite=fs, FsRead=fs):
            save_report!("q4", 4500)
            assert_equal(read_report!("q4"), "total: 4500", reason="round trip")
```

Paths are `Path` values, so `"reports/$name.txt"` is one argument, not
string surgery on separators. Integration tests get the real file system
and a fresh temporary directory per case; see
[Temporary Directories](../../spec/std/testing.md#temporary-directories).

## Scanning Logs With Regex

Operations read logs: count the server errors by status, or find the
minute that broke. `std.regex` compiles a pattern once and captures groups
by number or name; it runs in linear time, so a hostile log line cannot
stall the scan. The exact rules are in [Regex](../../spec/std/regex.md):

```hd
use std.regex.Regex
use std.text.r
fn status_of(line: string) -> i32?:
    match Regex::new(r"\"[A-Z]+ /[^ ]*\" (\d\d\d)"):
        .Ok(pattern) =>
            match pattern.captures(line):
                .Some(found) => found.get(1).map(fn(m): m.text.parse::[i32]().expect("digits"))
                .None => .None
        .Err(_) => .None

fn errors_by_status(lines: List[string]) -> Map[i32, usize]:
    let mut counts: Map[i32, usize] = {}
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

## Wiring A Program

An application's own traits, such as a `Mailer` and a `Repo`, are not host
capabilities: no host supplies them. Library code asks for them in its row,
`$ Mailer + Repo`, and `main` is the composition root. It builds the
concrete adapters and installs them with `$.with`, so its own row lists
only what the host supplies, here `Console`. A test is another
composition root: it installs fakes the same way, with no mocking library.
The rules are in
[Provider Scopes](../../spec/lang/11-requirements-and-suspension.md#provider-scopes):

```hd
trait Repo:
    fn save(&self, name: string) -> string

trait Mailer:
    fn send(&self, to: string) -> string

fn register(name: string, email: string) -> string $ Mailer + Repo:
    saved := $.use(Repo).save(name)
    sent := $.use(Mailer).send(email)
    "$saved; $sent"

struct FileRepo:
    root: string

impl Repo for FileRepo:
    fn save(&self, name: string) -> string: "file:${self.root}/$name"

struct SmtpMailer:
    endpoint: string

impl Mailer for SmtpMailer:
    fn send(&self, to: string) -> string: "smtp:${self.endpoint}:$to"

struct MemoryRepo:
    namespace: string

impl Repo for MemoryRepo:
    fn save(&self, name: string) -> string: "memory:${self.namespace}:$name"

struct OutboxMailer:
    label: string

impl Mailer for OutboxMailer:
    fn send(&self, to: string) -> string: "outbox:${self.label}:$to"

pub fn main() -> void $ Console:
    $.with(Mailer=&SmtpMailer { endpoint: "smtp.example.com" }, Repo=&FileRepo { root: "data/users" }):
        println(register("Ada", "ada@example.com"))

tests:
    it("registers with test adapters"):
        $.with(Mailer=&OutboxMailer { label: "test" }, Repo=&MemoryRepo { namespace: "case" }):
            assert_equal(register("Ada", "ada@example.com"), "memory:case:Ada; outbox:test:ada@example.com", reason="the installed fakes")
```

The production and the test adapters share no concrete type; each only
implements the trait. A provider is a trait value, so `$.with` takes it
behind a written `&`: `&FileRepo { ... }` makes one new object that the
whole block shares, and a provider that changes as it runs, such as a fake
that records its calls, takes `&mut` and a `&mut self` method. In a package, the traits and `register` go in a
library module, and each way of wiring them is its own executable. Each
`[[executable]]` table names an entry module, so `src/main.hd` wires
production and `src/dev.hd` wires the in-memory adapters
([Executables](../../spec/cli/command-line.md#executables)):

```toml
[package]
name = "accounts"

[[executable]]
name = "app"
module = "main"

[[executable]]
name = "dev"
module = "dev"
```

```text
# src/dev.hd
use pkg.{Mailer, MemoryRepo, OutboxMailer, Repo, register}

pub fn main() -> void $ Console:
    $.with(Mailer=&OutboxMailer { label: "local" }, Repo=&MemoryRepo { namespace: "dev" }):
        println(register("Ada", "ada@example.com"))
```

`hd run app` and `hd run dev` run the two. When one executable must
choose at startup, give each branch its own `$.with`. Its row then lists
every host capability either branch uses, such as `$ Console + Env` to
read `APP_ENV`, while `Mailer` and `Repo` stay out of it, because both
branches install them.

## Testing The Whole Program

A unit test runs on fakes; an integration test runs the real thing. Three
tools cover the rest of the job: `snapshot_file` compares rendered text
with a recorded file, `hd_run!` runs the package's own executable, and
`temp_dir` gives each test case a directory of its own that the runner
removes. The exact rules are in
[Snapshot Files](../../spec/std/testing.md#snapshot-files) and
[Running Executables](../../spec/std/testing.md#running-executables). In the
`shop` package, `src/report.hd` renders a report and
`tests/report.hd` checks it end to end:

```hd
use std.time.Timestamp

pub fn render(name: string, total: i32, at: Timestamp) -> string:
    "report $name: $total cents, at ${at.to_rfc3339()}"
```

```text
use std.process.Command
use std.testing.{hd_run, snapshot_file, temp_dir}
use std.time.Timestamp
use pkg.report.render

it("renders the quarterly report"):
    snapshot_file(render("q4", 4500, Timestamp::from_unix_milliseconds(0)))

it("runs the built program in its own directory"):
    let out = hd_run!(*Command::new("shop"))
    assert_equal(out.status, 0, reason="a clean exit")
    dir := temp_dir()
    assert_equal("$dir".len() > 0, true, reason="each case gets its own directory")
```

The first run fails until you record the snapshot with `hd test
--update`; every later run compares against it. A snapshot that should
change is re-recorded with another `--update`, and the diff is the review.
Tests that need deterministic randomness or arguments bind `SeededRandom`
or `ScriptedArgs` the same way as `MemoryFs`.

## Packages And Dependencies

A program larger than a file is a package: an `hd.toml` manifest beside a
`src/` folder. Each file under `src/` is a module named by its path, so
`src/shop/cart.hd` is `shop.cart`. A folder that holds modules needs a
`mod.hd`, the folder's own module, and its `pub use` lines list what the
folder offers. A `use` starts at one of five roots: `pkg` for this
package, `std` for the standard library, `dep.NAME` for a dependency, and
`self` and `super` for the current module and its parent. The rules are in
[Path-Inferred Modules](../../spec/lang/10-modules.md#path-inferred-modules)
and [Use Roots](../../spec/lang/10-modules.md#use-roots).

A path dependency names any local package, and neither package needs a
workspace. Create a library and an application side by side, then add
the library to the application:

```sh
hd new --lib money
hd new --app shop
cd shop
hd add ../money
```

`hd add` writes the requirement into `shop/hd.toml`, under the library's
package name:

```toml
[package]
name = "shop"

[dependencies]
money = { path = "../money" }
```

The library's `pub` items in `money/src/lib.hd` are its interface:

```hd
## An amount of money in whole cents.
pub struct Money:
    pub cents: i64

## The amount `cents`.
pub fn of_cents(cents: i64) -> Money:
    Money { cents: cents }

## The amount as dollars and cents, such as `$12.50`.
pub fn show(money: Money) -> string:
    rest := (money.cents % 100).to_string().pad_start(2, '0')
    "$${money.cents / 100}.$rest"
```

The application reaches it as `dep.money`, by the key in its manifest:

```text
# shop/src/main.hd
use dep.money.{of_cents, show}

pub fn main() -> void $ Console:
    println("total: ${show(of_cents(1250))}")
```

Run commands inside a package directory: in `shop`, `hd run` prints
`total: $12.50`.

A released package is a repository path and a minimum version, as
`json = "github.com/acme/json@2.1.0"`. `hd add acme/json` adds the newest
release of a GitHub repository, and `hd add acme/json@2.1` the newest 2.1
release. `hd update` moves every dependency to the newest release on its
compatibility line, `hd remove NAME` deletes one, and `hd fetch` fetches
what the cache lacks. They keep `hd.sum`, the recorded hashes, up to date;
commit it beside the manifest
([Dependency Commands](../../spec/cli/command-line.md#dependency-commands)).

A tagged release cannot hold a bare path requirement, since the path
means nothing in a fetched copy. A package that you release adds the
version a fetched copy should use, as Cargo does
([Path Requirements](../../spec/lang/10-modules.md#path-requirements)):

```toml
[dependencies]
money = { path = "../money", version = "0.4.2" }
```

Several packages of one repository can share one version selection and
one `hd.sum` through a workspace: an `hd.toml` at the root that lists the
members and declares no package. At the root, `hd test` tests every member
and `-p NAME` picks one, as in `hd run -p shop`
([Workspace Mode](../../spec/cli/command-line.md#workspace-mode)):

```toml
[workspace]
members = ["money", "shop"]
```

A workspace root has no package, so it has no `tasks/`. Tasks for the
whole repository go in a member of their own, such as `tools/`, as
Cargo's xtask pattern does: `hd run -p tools release` runs
`tools/tasks/release.hd`.

## Publishing A Package

A package others fetch needs a version and a hash everyone can check. A
version is a git tag: `v2.3.0` at the repository root, `lint/v2.3.0` for
a package in the `lint` subdirectory. A manifest never states its own
version. `hd.sum`, committed beside the manifest, records the hash of
each selected tree, and a fetched tree that differs is rejected. The
exact rules are in [Versions](../../spec/lang/10-modules.md#versions) and
[Integrity](../../spec/lang/10-modules.md#integrity):

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
before a build. Read next: [Dependencies](../../spec/cli/command-line.md#dependencies).

## Deeper Collections

`List` and `Map` cover most days. Three more shapes cover the rest: `Set`
for distinct values, `sorted` and `group_by` for ordering and bucketing,
and the adapters on any iterator — `filter`, `map`, `fold`. The exact
rules are in [Collections](../../spec/std/collections.md) and
[Iterators](../../spec/std/iter.md):

```hd
use std.collections.Set
struct Item:
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
        let mut seen: Set[string] = Set::new()
        let mut kept: List[string] = []
        for item in sample():
            for tag in item.tags:
                if seen.insert(tag):
                    kept.push(tag)
        assert_equal(kept, ["fruit", "fresh", "bakery", "dairy"], reason="first occurrences, in item order")
```

A `Set` keeps each value once, in the order it first appeared. `Deque`
and `Heap` serve a queue and a priority queue; see
[Collections](../../spec/std/collections.md).

## Points And Intervals In Time

Two types cover the clock: a `Timestamp` is a point in UTC time, and a
`Duration` is a distance between two points. Text in and out is RFC 3339,
so a config or a log line reads the same everywhere. An `Instant` is a
monotonic reading for measuring intervals, never a wall time. The exact
rules are in [Time](../../spec/std/time.md):

```hd
use std.time.{Duration, Instant, Timestamp, min, s}

fn deadline(now: Timestamp) -> Timestamp:
    now + 30s

fn elapsed(start: Instant, end: Instant) -> Duration:
    end.since(start)

tests:
    it("reads and writes RFC 3339 text"):
        at := "2026-10-06T12:00:00Z".parse::[Timestamp]().expect("valid")
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
character or by byte. The exact rules are in [Text](../../spec/std/text.md):

```hd
use std.text.StringBuilder

fn csv_row(cells: List[string]) -> string:
    cells.join(",")

fn banner(text: string) -> string:
    out := StringBuilder::new()
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
