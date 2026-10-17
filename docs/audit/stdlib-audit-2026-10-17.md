# Standard library audit: common tasks and batteries, 2026-10-17

Issue #350, job A. This audit checks `lib/std` against the owner's
principles 1 to 5: batteries included, good library design (Go, Effect,
Rust), hd's own features, ergonomic calls for the common 80%, and
composable atoms for the rest. It writes 32 common tasks as hd programs
and compares each with Go and Python. Then it lists the missing batteries
and the design problems. It changes no code. Job B covers performance in
[stdlib-impl-review-2026-10-17.md](stdlib-impl-review-2026-10-17.md).

|  |  |
| --- | --- |
| Commit | `c904f58f6`, with `target/debug/hd` built from it |
| Gate | every program below that is not marked `# proposed` passes `hd check` on that build (`--tests` for task 31) |
| Runs | every program that needs no network ran. Task 4 ran against a local `python3 -m http.server`, and task 5 under `ScriptedHttp` |
| Programs | `target/stdlib-audit/t/` (not committed) |
| Line counts | non-blank lines of a whole program. Go and Python counts are estimates of a whole minimal program with errors handled as in hd |

## Summary

The owner's counter-example, an HTTP response's text, is already one
line, `get!(url)?.text()`, but it treats a 404 as success. Decoding JSON
into a type is one line. Errors chain with `.context(...)` and
print as a cause chain. A test swaps any host capability with
`$.with(...)`. 25 of the 32 programs checked on the first try, and 31 by
the second. hd is shorter than Go in 29 tasks and longer than Python in
30.

The gaps are missing shortcuts on host calls (most `std.fs` methods and
all of `std.process` lack free helpers, and HTTP treats a 404 as
success), missing atoms (float math, walk and glob, `Duration` parsing,
URL encoding, CSV, HMAC, timeouts), hd features that stop short (JSON
decoding ignores declared defaults), and small inconsistencies.

### The 10 highest-value changes

Ranked by user value per cost. A **shortcut** is principle 4, an
ergonomic call for the common case. An **atom** is principle 5, a
missing building block. Cost: S is an afternoon in hd; M is a day or a
spec section; L needs an owner decision, a host hook or an intrinsic.

| # | Change | Kind | Cost | Tasks |
| --- | --- | --- | --- | --- |
| 1 | HTTP status as an error: `Response.ok() -> Result[Response, HttpError]` with `HttpError.Status(status, url)`, plus `get_text!(url)`, `Response.json::[T]()` and `post_json!(url, value)` | shortcut | S | 4, 5, 6, 28 |
| 2 | A free helper for every host-trait method: `list_dir!`, `stat!`, `read_bytes!`, `append_text!`, `create_dir_all!`, `remove!`, `rename!`, `exists!`, and `std.process.run!(program, args, stdin = "")` | shortcut | S | 1, 2, 3, 10, 30 |
| 3 | `std.error.message(text) -> dyn Error` for one-off errors, and `Option.context(message)` | shortcut | S | 5, 24 |
| 4 | Number math: integer `abs`, `pow`, `signum`; float `abs`, `floor`, `ceil`, `round`, `trunc`, `min`, `max`, `sqrt`, `PI` | atom | M; L for `sqrt` | every numeric program |
| 5 | `std.fs.walk!(dir)` and `glob!(pattern)` | atom, shortcut | S | 3 |
| 6 | Collection and text atoms: `Set` collects, `Map.entries()`, `max_by_key`, `min_by_key`, `dedup`, `most_common(n)`, `string.rfind`, `splitn`, `rsplit_once` | atom | S | 18, 19, 20 |
| 7 | `std.task.timeout!(after, task)`, `map_all!(items, limit, f)`, and a `when` predicate on `retry!` and `retry_with!` | atom | S to M | 26, 27, 28 |
| 8 | Time: free `monotonic()` and `elapsed(start)`, `Duration::parse`, `Date.to_iso8601()`, `Timestamp.format(pattern)`, nanosecond `Duration` | shortcut, atom | M; L for nanoseconds | 21, 22, 23 |
| 9 | JSON: decode fills a missing key from the member's default; `Json.pointer`, `Json.as_i64()`, `encode_pretty` | shortcut | M | 7, 8, 9 |
| 10 | A `Parse` trait (Rust's `FromStr`) with `string.parse::[T]()`, so `std.cli` and `std.host` can offer `value_as::[T]` and `env_as::[T]` | atom | M | 11, 12 |

Changes 1, 2, 3, 5 and 6 are plain hd within the stdlib calls the owner
has delegated. `sqrt` and a nanosecond clock need the owner.

## Task table

"std names" lists what a program imports or calls beyond the prelude,
and its row.

| # | Task | hd | Go | Py | std names and row | Hard or missing | Proposal |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Read a file, write one, append | 9 | 20 | 4 | `read_text!`, `write_text!`, `Path`, `$.use(FsWrite).append_text!` | no `append_text!` helper | change 2 (shortcut) |
| 2 | Does a file exist, how big | 7 | 13 | 4 | `$.use(FsRead).stat!` | no `stat!` or `exists!` | change 2 (shortcut) |
| 3 | Walk a tree, keep `*.md` | 16 | 18 | 3 | `$.use(FsRead).list_dir!`, `EntryKind`, `extension` | hand recursion | `walk!`, `glob!` (atom, shortcut) |
| 4 | GET a URL's text | 5 | 16 | 3 | `get!`, `text`; `$ Http` | a 404 is `.Ok` | `get_text!` (shortcut) |
| 5 | POST JSON, decode the reply | 29 | 30 | 6 | `send!`, `Request`, `Method`, `encode`, `decode`, `Error` | body bytes and header by hand; an error type for the status | `post_json!`, `ok()`, `json` (shortcut) |
| 6 | Header, timeout, status | 13 | 22 | 6 | `send!`, `Request`, `s` | manual status check | `ok()` (shortcut) |
| 7 | Decode JSON into a type | 15 | 22 | 6 | `decode`, `Deserialize` | member defaults ignored | defaults (atom) |
| 8 | Encode JSON, compact and pretty | 14 | 20 | 5 | `encode`, `to_json(...).pretty()` | two calls for pretty | `encode_pretty` (shortcut) |
| 9 | One field of unknown JSON | 8 | 15 | 3 | `parse`, `get`, `at`, `as_number`, `as_i64` | a lambda per step | `pointer`, `as_i64` (shortcut) |
| 10 | Run a command, check the exit | 8 | 14 | 4 | `$.use(Process).run!(p, args, "")` | no helper; stdin required | `run!`, `output!` (shortcut) |
| 11 | Parse flags and arguments | 12 | 14 | 8 | `Cli`, `parse_args`, `parse_usize`; `$ Args` | `--help` declared by hand; values are text | `value_as` (atom) |
| 12 | Read env, exit with a code | 10 | 16 | 7 | `env`, `args`, `ExitCode`, `eprintln` | nothing | none |
| 13 | Count words on stdin | 10 | 14 | 3 | `read_line!`; `$ ConsoleInput` | no read-all | `read_all!` (shortcut) |
| 14 | Parse `key = value` lines | 11 | 20 | 9 | `lines`, `trim`, `split_once`, `replace` | nothing | none |
| 15 | Table, decimals, digit groups | 23 | 16 | 6 | `pad_start`, `pad_end`, `to_fixed`, `slice` | no grouping | `format_grouped` (nice) |
| 16 | Build a string | 9 | 14 | 2 | `StringBuilder`, `join` | nothing | none |
| 17 | Extract fields with a regex | 11 | 16 | 6 | `Regex`, `captures_all`, `r` | `r"..."` needs an import | owner question |
| 18 | Word frequency, top 3 | 6 | 22 | 3 | `counts`, `most_common` | nothing (was pairs by hand) | `entries`, `most_common` (applied, #368) |
| 19 | Group, sort by key, dedupe | 12 | 28 | 10 | `group_by`, `min_by_key`, `sum`, `unique` | nothing (was dedupe by hand) | `min_by_key`, `unique` (applied, #368) |
| 20 | Top k with a heap | 12 | 30 | 3 | `Heap`, `Reverse` | nothing | `top_k` (nice) |
| 21 | Time a block | 12 | 12 | 6 | `$.use(Clock).monotonic()`, `since` | ms resolution; no helper | `elapsed`, ns (shortcut, atom) |
| 22 | Format today's date | 7 | 8 | 3 | `now`, `date`, `pad_start` | `Date` has no text form | `to_iso8601`, `format` (shortcut) |
| 23 | Parse `1h30m15s` | 24 | 9 | 12 | `h`, `min`, `s`, `ms`, `parse_i64` | no parser | `Duration::parse` (atom) |
| 24 | Wrap errors with context | 15 | 25 | 10 | `context`, `dyn Error` | nothing | none |
| 25 | Typed error enum with causes | 17 | 30 | 15 | `@error`, `@from`, `@source`, `map_err` | nothing | none |
| 26 | Run N tasks together | 11 | 22 | 8 | `all_list!` | typed closure list; no limit | `map_all!` (atom) |
| 27 | Time out one task | 12 | 20 | 10 | `race!`, `sleep!` | a sleeper per result type | `timeout!` (atom) |
| 28 | Retry with backoff | 9 | 22 | 12 | `retry_with!`, `Backoff` | retries every error, never a 503 | `when`, `ok()` |
| 29 | Random draws, secure token | 8 | 12 | 6 | `rng`, `int`, `shuffle`, `Random.fill`, `hex_encode` | token needs the raw provider | `token_hex` (nice) |
| 30 | SHA-256 of a file, base64, hex | 13 | 24 | 7 | `$.use(FsRead).read_bytes!`, `sha256_hex`, `base64_*`, `hex_encode` | no URL-safe base64 | `base64url_*` (atom) |
| 31 | Table test, property test | 14 | 30 | 14 | `it_each`, `it_prop`, `assert_equal` | named `body=` with the row type | owner question |
| 32 | Test with a fake clock | 15 | 25 | 12 | `ManualClock`, `$.with`, `Timestamp` | nothing | none |

Totals: hd 397 lines, Go about 620, Python about 215. hd beats Go where
Go spends lines on `if err != nil` and `container/heap`. hd loses where a
one-call battery exists: Go's `time.ParseDuration` and `text/tabwriter`,
Python's `glob`, `Counter.most_common`, `heapq.nlargest` and format specs.

## Per-task notes

Each task shows its program as it checks today. A block that starts with
`# proposed` uses calls that do not exist yet and does not check.

### Files and paths

**1. Read, write, append.** `append_text!` is a free helper. Go: `os.ReadFile`, `os.WriteFile`,
`os.OpenFile` with `O_APPEND`. Python: `open(p).read()`, `open(p, "a")`.

```hd
use std.fs.{FsRead, FsWrite, FsError, append_text, read_text, write_text}
use std.path.Path

pub fn main!() -> Result[void, FsError] $ Console + FsRead + FsWrite:
    text := read_text!(Path("notes.txt"))?
    lines := text.lines().filter(fn(l: string) -> bool: !l.trim().is_empty())
    out := Path("out.txt")
    write_text!(out, "${lines.len()} non-empty lines\n")?
    append_text!(out, "done\n")?
    .Ok(())
```

**2. Exists and size.** `stat!` (`.None` when missing) and `exists!` are
free helpers. Go: `os.Stat`, `errors.Is(err, fs.ErrNotExist)`. Python:
`os.path.exists`, `getsize`.

```hd
use std.fs.{FsRead, FsError, stat}
use std.path.Path

pub fn main!() -> Result[void, FsError] $ Console + FsRead:
    match stat!(Path("notes.txt"))?:
        .Some(entry) => println("exists, ${entry.size} bytes")
        .None => println("missing")
    .Ok(())
```

**3. Walk and glob.** Every tree walk repeated a hand recursion, and
`Entry` has no modified time. `walk!` is the atom (Go's
`filepath.WalkDir`), `glob!` the shortcut (Python's
`glob.glob("tree/**/*.md", recursive=True)`); both are plain hd over
`list_dir!` and `stat!` (added in #367).

```hd
use std.fs.{EntryKind, FsError, FsRead, walk}
use std.path.Path

pub fn main!() -> Result[void, FsError] $ Console + FsRead:
    for entry in walk!(Path("tree"))?:
        if entry.kind == EntryKind.File && entry.path.extension() == .Some("md"):
            println(entry.path)
    .Ok(())
```

```hd
use std.fs.{FsError, FsRead, glob}

pub fn main!() -> Result[void, FsError] $ Console + FsRead:
    for file in glob!("tree/**/*.md")?:
        println(file)
    .Ok(())
```

### HTTP

**4. GET a URL's text.** One line with `get_text!`. Before it, `get!(url)?.text()`
ran against a local server and, at a missing path, printed the server's
HTML 404 page and exited 0. Now `Response.ok()` makes a status outside
200 to 299 an `.Err(HttpError.Status(status, url))`, as reqwest's
`error_for_status`, and `get_text!(url)` is `get!(url)?.ok()?.text()`.
Go: `http.Get`, a `StatusCode` check, `io.ReadAll`. Python:
`urlopen(url).read().decode()`, which raises on 404.

```hd
use std.http.{Http, HttpError, get_text}

pub fn main!() -> Result[void, HttpError] $ Console + Http:
    println(get_text!("http://127.0.0.1:8765/notes.txt")?)
    .Ok(())
```

**5. POST JSON, decode the reply.** The longest program: the body is
bytes, the content type is set by hand, and a bad status needs a
seven-line error type. `?` converts `HttpError` and `JsonError` into
`dyn Error` by itself. Go: about 30 lines. Python: `requests.post(url,
json=body).json()`.

```hd
use std.http.{Http, HttpError, Method, Request, send}
use std.json.{JsonError, decode, encode}
use std.serde.{Deserialize, Serialize}
use std.error.Error

@derive(Serialize)
data NewUser:
    name: string

@derive(Deserialize)
data Created:
    id: i64
    name: string

pub fn main!() -> Result[void, dyn Error] $ Console + Http:
    request := Request {
        method: Method.Post,
        url: "https://api.example.com/users",
        headers: [("Content-Type", "application/json")],
        body: encode(NewUser { name: "Ada" }).to_utf8(),
    }
    response := send!(request)?
    if response.status >= 300:
        return .Err(StatusError { status: response.status })
    created := decode::[Created](response.text())?
    println("created ${created.id} ${created.name}")
    .Ok(())

data StatusError:
    status: u16

impl Display for StatusError:
    fn to_string(self) -> string: "http status ${self.status}"

impl Error for StatusError
```

With `post_json!`, `ok()` and `json`:

```hd
use std.http.{Http, post_json}
use std.serde.{Deserialize, Serialize}
use std.error.Error

@derive(Serialize)
data NewUser:
    name: string

@derive(Deserialize)
data Created:
    id: i64
    name: string

pub fn main!() -> Result[void, dyn Error] $ Console + Http:
    response := post_json!("https://api.example.com/users", NewUser { name: "Ada" })?
    created := response.ok()?.json::[Created]()?
    println("created ${created.id} ${created.name}")
    .Ok(())
```

**6. Header, timeout, status.** The `Request` literal with defaults
reads well; only the status check is manual, and there is no query
encoding. Go: `http.NewRequest`, `Header.Set`, `Client{Timeout}`.
Python: `urlopen(Request(url, headers=h), timeout=5)`.

```hd
use std.http.{Http, HttpError, Request, send}
use std.time.s

pub fn main!() -> Result[void, HttpError] $ Console + Http:
    response := send!(Request {
        url: "https://api.example.com/me",
        headers: [("Authorization", "Bearer token123")],
        timeout: .Some(s(5)),
    })?
    if response.status != 200:
        println("failed: ${response.status}")
        return .Ok(())
    println(response.header("content-type").unwrap_or("?"))
    .Ok(())
```

### JSON

**7. Decode into a type.** One line, and errors name the path (`expected
u16 at $.port`). But `port: u16 = 8080` still fails with `MissingField`
when the key is absent, though `Field.default()` exists
(`lib/std/structure.hd:133`); proposed: decode uses member defaults. Go:
`json.Unmarshal` into a pre-filled struct. Python: `json.load`.

```hd
use std.fs.{FsRead, read_text}
use std.path.Path
use std.json.decode
use std.serde.Deserialize
use std.error.Error

@derive(Deserialize)
data Config:
    name: string
    port: u16
    tags: List[string]
    debug: bool?

pub fn main!() -> Result[void, dyn Error] $ Console + FsRead:
    config := decode::[Config](read_text!(Path("config.json"))?)?
    println("${config.name} on ${config.port}, debug=${config.debug.unwrap_or(false)}")
    .Ok(())
```

**8. Encode.** Pretty text takes two calls; proposed
`encode_pretty(shape)`, serde_json's `to_string_pretty`. Go:
`json.MarshalIndent`. Python: `json.dumps(v, indent=2)`.

```hd
use std.json.{encode, to_json}
use std.serde.Serialize

@derive(Serialize)
data Point:
    x: i32
    y: i32

@derive(Serialize)
data Shape:
    name: string
    points: List[Point]

pub fn main() -> void $ Console:
    shape := Shape { name: "tri", points: [Point { x: 0, y: 0 }, Point { x: 3, y: 4 }] }
    println(encode(shape))
    println(to_json(shape).pretty())
```

**9. One field of unknown JSON.** A lambda per step, and two hops per
number. Proposed: `doc.pointer("/db/pool/max").and_then(fn(m):
m.as_i64())`, RFC 6901 as in serde_json. Go: a type assertion per level.
Python: `doc["db"]["pool"]["max"]`.

```hd
use std.json.{JsonError, parse}

pub fn main() -> Result[void, JsonError] $ Console:
    doc := parse("{\"db\":{\"pool\":{\"max\":10}},\"tags\":[\"a\",\"b\"]}")?
    max := doc.get("db").and_then(fn(d): d.get("pool")).and_then(fn(p): p.get("max")).and_then(fn(m): m.as_number()).and_then(fn(n): n.as_i64())
    println("max = ${max.unwrap_or(0)}")
    tag := doc.get("tags").and_then(fn(t): t.at(0)).and_then(fn(t): t.as_text())
    println("tag = ${tag.unwrap_or("")}")
    .Ok(())
```

### Processes and the command line

**10. Run a command.** `run!(program, args, stdin = "")` is a free
helper, with a manual status check. Proposed: `output!(program, args) -> Result[string, ProcessError]` with a non-zero
exit as `.Err(ProcessError.Exited(status, stderr))`. Go:
`exec.Command(...).Output()`. Python: `subprocess.check_output`.

```hd
use std.process.{Process, ProcessError, run}

pub fn main!() -> Result[void, ProcessError] $ Console + Process:
    output := run!("git", ["rev-parse", "--short", "HEAD"])?
    if output.status != 0:
        println("git failed (${output.status}): ${output.stderr.trim()}")
        return .Ok(())
    println("commit ${output.stdout.trim()}")
    .Ok(())
```

**11. Flags and arguments.** The builder and `usage()` work well, but an
undeclared `--help` is `unknown option --help`, and values are text.
Proposed: `parsed.value_as::[usize]("max")?`, over change 10. Go: typed
`flag.Int`, automatic `-h`. Python: `argparse` with `type=int`.

```hd
use std.cli.{Cli, CliError}
use std.host.Args
use std.num.parse_usize

pub fn main() -> Result[void, CliError] $ Console + Args:
    cli := Cli::new("grep-lite").flag("help", .Some('h'), "show help").flag("ignore-case", .Some('i'), "ignore case").option("max", .Some('m'), "stop after N matches").positional("pattern", "text to find")
    parsed := cli.parse_args()?
    if parsed.flag("help"):
        println(cli.usage())
        return .Ok(())
    max := parsed.value("max").and_then(fn(text): parse_usize(text).ok()).unwrap_or(10)
    println("pattern=${parsed.positionals[0]} icase=${parsed.flag("ignore-case")} max=$max")
    .Ok(())
```

**12. Env and exit code.** Nothing to add. Only `main` picks the code; a
returned `.Err` exits 1 after the cause chain. Go: `os.Getenv`,
`os.Exit(2)`. Python: `os.environ.get`, `sys.exit(2)`.

```hd
use std.process.ExitCode
use std.host.{Args, Env, args, env}
use std.console.eprintln

pub fn main() -> ExitCode $ Console + Args + Env:
    if args().is_empty():
        eprintln("usage: tool FILE")
        return ExitCode(2)
    level := env("LOG_LEVEL").unwrap_or("info")
    println("file=${args()[0]} level=$level")
    ExitCode(0)
```

**13. Standard input.** Clear; a `read_all!()` would make it three
lines, as `sys.stdin.read()` and Go's `io.ReadAll(os.Stdin)` do.

```hd
use std.console.{ConsoleInput, read_line}

pub fn main!() -> Result[void, ConsoleError] $ Console + ConsoleInput:
    let total: i64 = 0
    while true:
        line := match read_line!()?:
            .Some(text) => text
            .None => break
        total = total + i64(line.split_whitespace().len())
    println("$total words")
    .Ok(())
```

### Text

**14. Parse `key = value` lines.** Covered, with Rust's names. Go:
`strings.Cut`. Python: `partition("=")`.

```hd
pub fn main() -> void $ Console:
    config := "# settings\nname = demo\n  port=8080  \nbad line\n"
    let values: mut Map[string, string] = {}
    for line in config.lines():
        line := line.trim()
        if line.is_empty() || line.starts_with("#"):
            continue
        match line.split_once("="):
            .Some((key, value)) => values[key.trim()] = value.trim()
            .None => println("skipped: $line")
    println(values.get("port").unwrap_or("?").replace("80", "90"))
```

**15. Table, decimals, digit groups.** Padding and `to_fixed` work;
grouping is a hand helper, as in Go. Go: `text/tabwriter`, `%8.2f`.
Python: `f"{price:>8.2f}"`, `f"{total:,}"`.

```hd
data Row:
    name: string
    qty: i32
    price: f64

pub fn main() -> void $ Console:
    rows := [Row { name: "tea", qty: 3, price: 2.5 }, Row { name: "biscuits", qty: 12, price: 0.333 }]
    let width = 4
    for row in rows:
        if row.name.chars().count() > width:
            width = row.name.chars().count()
    println("${"item".pad_end(width)}  ${"qty".pad_start(5)}  ${"price".pad_start(8)}")
    for row in rows:
        println("${row.name.pad_end(width)}  ${"${row.qty}".pad_start(5)}  ${row.price.to_fixed(2).pad_start(8)}")
    total := 1234567
    println("total: ${group_thousands("$total")}")

fn group_thousands(digits: string) -> string:
    let parts: mut List[string] = []
    let end = digits.len()
    while end > 3:
        parts.push(digits.slice(end - 3, end))
        end = end - 3
    parts.push(digits.slice(0, end))
    parts.reversed().join(",")
```

**16. Build a string.** Both forms are short. Go: `strings.Builder`.
Python: `", ".join(...)`.

```hd
use std.text.StringBuilder

pub fn main() -> void $ Console:
    let out: mut StringBuilder = StringBuilder::new()
    for i in 0..5:
        if i > 0:
            out.push(", ")
        out.push("item$i")
    println(out.build())
    println([for i in 0..5 => "item$i"].join(", "))
```

**17. A regex.** Named groups and `captures_all` work. Without `use
std.text.r` the raw strings are ``unknown-name: `r` is not defined``;
the prelude is language tier, so that is an owner question. The deferred
`(?i)` and `\b` hurt log searches most. Go: `FindAllStringSubmatch`.
Python: `re.finditer`.

```hd
use std.regex.{Regex, RegexError}
use std.text.r

pub fn main() -> Result[void, RegexError] $ Console:
    log := "2026-10-17 ERROR disk full\n2026-10-17 INFO ok\n2026-10-18 ERROR net down"
    re := Regex::new(r"(?P<date>\d{4}-\d{2}-\d{2}) ERROR (?P<msg>.*)")?
    for caps in re.captures_all(log):
        date := caps.name("date").map(fn(m): m.text).unwrap_or("")
        msg := caps.name("msg").map(fn(m): m.text).unwrap_or("")
        println("$date: $msg")
    println(Regex::new(r"\s+")?.replace_all("a   b \t c", " "))
    .Ok(())
```

### Collections

**18. Word frequency.** `most_common` on the tally replaces the hand-built
pairs and the sort (`Map.entries()` gives the pairs when a program needs
them). Python: `Counter.most_common`. Go: a pair slice and `sort.Slice`.

```hd
use std.collections.counts

pub fn main() -> void $ Console:
    text := "the cat and the hat and the bat"
    tally := counts(text.split_whitespace())
    for (word, n) in tally.most_common(3):
        println("$word $n")
```

**19. Group, sort, dedupe.** `group_by` and `sorted_by_key` read well.
`min_by_key` replaces the sort for the cheapest order, and `unique()`
replaces the dedupe loop. The task needs whole-list uniqueness (the
repeated customer is not adjacent), which is `unique`; `dedup` is
Rust's adjacent-only `Vec::dedup`. Python: `dict.fromkeys(names)`.

```hd
data Order:
    customer: string
    cents: i64

pub fn main() -> void $ Console:
    orders := [Order { customer: "bo", cents: 300 }, Order { customer: "al", cents: 120 }, Order { customer: "bo", cents: 50 }]
    by_customer := orders.group_by(fn(o): o.customer)
    for (name, list) in by_customer:
        println("$name: ${list.map(fn(o): o.cents).sum()}")
    match orders.min_by_key(fn(o): o.cents):
        .Some(cheapest) => println(cheapest.customer)
        .None => pass
    println(orders.map(fn(o): o.customer).unique().join(","))
```

**20. Top k.** 12 lines with `Heap` and `Reverse`, against 30 for Go's
`container/heap`. Python: `heapq.nlargest(3, scores)`.

```hd
use std.collections.Heap
use std.cmp.Reverse

pub fn main() -> void $ Console:
    scores := [+42, 7, 99, 13, 58, 61, 3]
    let heap: mut Heap[Reverse[i32]] = Heap::new()
    for s in scores:
        heap.push(Reverse { value: s })
        if heap.len() > 3:
            _ := heap.pop()
    top := heap.to_sorted_list().map(fn(r): r.value)
    println(debug(top))
    println(debug(scores.sorted().reversed().take(3)))
```

### Time

**21. Time a block.** `monotonic()` has no free helper, and `Duration`
holds milliseconds, so this prints `1ms`. Proposed: `start :=
monotonic()`, `elapsed(start)`, nanosecond `Duration`. Go: `time.Since`.
Python: `perf_counter()`.

```hd
use std.time.Clock

fn work() -> i64:
    let sum: i64 = 0
    for i in 0..100000:
        sum = sum + i64(i)
    sum

pub fn main() -> void $ Console + Clock:
    clock := $.use(Clock)
    start := clock.monotonic()
    result := work()
    elapsed := clock.monotonic().since(start)
    println("sum=$result in $elapsed")
```

**22. Format a date.** `Date` has no text form (no `Display`, by a
confirmed call), and no hours, weekday, zone or pattern. Proposed:
`date.to_iso8601()` and `now().format("%Y-%m-%d %H:%M")`. Go:
`t.Format("2006-01-02")`. Python: `isoformat()`.

```hd
use std.time.{Clock, now}

fn two(n: i32) -> string: "$n".pad_start(2, '0')

pub fn main() -> void $ Console + Clock:
    stamp := now()
    date := stamp.date()
    println("${date.year}-${two(date.month)}-${two(date.day)}")
    println(stamp.to_rfc3339())
```

**23. Parse a duration.** No parser, so a config's `"1h30m"` costs 20
lines. Proposed: `Duration::parse(text)?`, reading Go's units and the
`min` that `Display` writes, and `Deserialize` from text. Go:
`time.ParseDuration`. Python: none.

```hd
use std.time.{Duration, h, min, s, ms}
use std.num.parse_i64

fn parse_duration(text: string) -> Duration?:
    let total = ms(0)
    let digits = ""
    for c in text.chars():
        if c.is_ascii_digit():
            digits = digits + "$c"
            continue
        n := parse_i64(digits).ok()?
        digits = ""
        unit := match c:
            'h' => h(n)
            'm' => min(n)
            's' => s(n)
            _ => return .None
        total = total + unit
    if !digits.is_empty():
        return .None
    .Some(total)

pub fn main() -> void $ Console:
    match parse_duration("1h30m15s"):
        .Some(d) => println("$d = ${d.as_milliseconds()} ms")
        .None => println("bad duration")
```

### Errors

**24. Context and report.** A strength: the run printed `loading
settings`, `caused by: reading missing.json`, `caused by: not found:
missing.json`. A one-off error is `.Err(message("..."))`, as Go's
`errors.New` and anyhow's `anyhow!`, and `Option.context` turns a `.None`
into one (both added in #366, below). Python: `raise ... from err`.

```hd
use std.fs.{FsRead, read_text}
use std.path.Path
use std.json.decode
use std.serde.Deserialize
use std.error.{Error, message}

@derive(Deserialize)
data Settings:
    port: u16

fn load!(path: Path) -> Result[Settings, dyn Error] $ FsRead:
    text := read_text!(path).context("reading $path")?
    settings := decode::[Settings](text).context("parsing $path")?
    if settings.port == 0:
        return .Err(message("port 0 is reserved"))
    .Ok(settings)

fn first_arg(args: List[string]) -> Result[string, dyn Error]:
    args.first().context("no config path given")

pub fn main!() -> Result[void, dyn Error] $ Console + FsRead:
    path := first_arg(["missing.json"])?
    settings := load!(Path(path)).context("loading settings")?
    println("port ${settings.port}")
    .Ok(())
```

With #366 the one-off error needs no type and no impl: `message` and
`Option.context` replace the seven-line `StatusError` of task 5.

**25. A typed error enum.** `@error` with `@from` and `@source` is
thiserror in hd; the run printed `line 3 is not a number`, `caused by:
invalid digit at position 0`. Go: a struct and `Unwrap()` per error.

```hd
use std.fs.{FsRead, FsError, read_text}
use std.path.Path
use std.num.{ParseNumberError, parse_i64}

@error
pub enum AppError:
    @error("cannot read input")
    Read(@from source: FsError)
    @error("line $line is not a number")
    Parse(line: usize, @source source: ParseNumberError)

fn total!(path: Path) -> Result[i64, AppError] $ FsRead:
    let sum: i64 = 0
    for (i, line) in read_text!(path)?.lines().iter().enumerate():
        sum = sum + parse_i64(line.trim()).map_err(fn(e): AppError.Parse(i + 1, e))?
    .Ok(sum)

pub fn main!() -> Result[void, AppError] $ Console + FsRead:
    println("total ${total!(Path("numbers.txt"))?}")
    .Ok(())
```

### Concurrency

**26. Run N tasks.** Sleeps of 100, 200 and 300 ms finished in 0.38 s.
`std.task.map_all!` takes the items, a limit and the function, and keeps
the results in input order (Effect's `forEach` with `concurrency`, Go's
`errgroup.SetLimit`). Rewritten with it, the program checks and prints the
same three lines in 0.39 s.

```hd
use std.task.map_all
use std.time.{Clock, ms, sleep}

fn job!(id: i64) -> string $ Clock:
    sleep!(ms(100 * id))
    "job $id done"

pub fn main!() -> void $ Console + Clock:
    for line in map_all!([1, 2, 3], 8, job):
        println(line)
```

**27. Time out one task.** `std.task.timeout!` races the task against a
sleeper and returns `Result[T, TimedOut]`, so the task's own result type
needs no sleeper of its own. Go: `context.WithTimeout`. Python:
`asyncio.wait_for`. Rewritten with it, the program checks and prints
`timed out after 200ms`.

```hd
use std.task.timeout
use std.time.{Clock, ms, s, sleep}

fn slow_lookup!() -> Result[string, string] $ Clock:
    sleep!(s(5))
    .Ok("value")

pub fn main!() -> void $ Console + Clock:
    match timeout!(ms(200), slow_lookup):
        .Ok(.Ok(value)) => println(value)
        .Ok(.Err(why)) => println(why)
        .Err(timed_out) => println(timed_out.to_string())
```

**28. Retry with backoff.** `retry_with!` takes a `when` predicate, as
Effect's `Schedule.whileInput` does, so only transient errors are retried.
The predicate is a plain function in the program, since `HttpError` has no
`is_transient` method. Rewritten with it, the program checks.

```hd
use std.task.{Backoff, retry_with}
use std.time.{Clock, ms, s}
use std.http.{Http, HttpError, get}

fn is_transient(error: HttpError) -> bool:
    match error:
        .Timeout => true
        .Connect(_) => true
        .Dns(_) => true
        .Status(status, _) => status == 429 || status >= 500
        _ => false

pub fn main!() -> Result[void, HttpError] $ Console + Clock + Http:
    policy := Backoff { attempts: 4, initial: ms(100), factor: 2, max: s(2) }
    response := retry_with!(policy, when=is_transient):
        get!("https://api.example.com/health")
    println("status ${response?.status}")
    .Ok(())
```

### Randomness, hashing, encoding

**29. Random draws and a token.** A secret must come from the provider,
since `Rng` is not cryptographic, and nothing says so. Proposed: a doc
line and `token_hex(bytes) $ Random`. Go: `crypto/rand`. Python:
`secrets.token_hex`.

```hd
use std.random.{Random, rng}
use std.encoding.hex_encode

pub fn main() -> void $ Console + Random:
    let mut gen = rng()
    roll := gen.int(1..=6)
    let deck: mut List[string] = ["A", "K", "Q", "J"]
    gen.shuffle(deck)
    token := hex_encode($.use(Random).fill(16))
    println("roll=$roll deck=${deck.join("")} token=$token")
```

**30. SHA-256, base64, hex.** One call each; the digest matched `shasum
-a 256`. Missing: URL-safe base64 and HMAC. Go: `sha256.Sum256`,
`base64.StdEncoding`. Python: `hashlib`, `base64`.

```hd
use std.fs.FsRead
use std.path.Path
use std.digest.sha256_hex
use std.encoding.{base64_decode, base64_encode, hex_encode}
use std.error.Error

pub fn main!() -> Result[void, dyn Error] $ Console + FsRead:
    bytes := $.use(FsRead).read_bytes!(Path("notes.txt"))?
    println("${sha256_hex(bytes)}  notes.txt")
    encoded := base64_encode("user:secret".to_utf8())
    println("Basic $encoded")
    println(string::from_utf8(base64_decode(encoded)?)?)
    println(hex_encode([0xde, 0xad, 0xbe, 0xef]))
    .Ok(())
```

### Testing

**31. Table and property tests.** Property tests are built in (Go's
`testing/quick` is frozen; Python needs Hypothesis). But `it_each` and
`it_prop` need `body=fn!(row: (A, B)):` and a closing `)`, unlike `it`:
an owner question. A file named `*_test.hd` is a test module, so its
`tests:` block is `misplaced-tests-block`, with a clear message.

```hd
use std.encoding.{base64_decode, base64_encode}

fn slug(title: string) -> string:
    title.trim().lower().split_whitespace().join("-")

pub fn main() -> void $ Console:
    println(slug("Hello World"))

tests:
    use std.testing.{assert_equal, it_each, it_prop}

    it_each("slugs", [("Hello World", "hello-world"), ("  Trim  me ", "trim-me"), ("", "")], body=fn!(row: (string, string)):
        let (input, expected) = row
        assert_equal(slug(input), expected, reason="slug of $input")
    )

    it_prop("base64 round trips", prop=fn!(bytes: List[u8]):
        assert_equal(base64_decode(base64_encode(bytes)), .Ok(bytes), reason="round trip")
    )
```

**32. Fake clock.** The cleanest test here: the code names `$ Clock` and
the test binds a `ManualClock`. Go needs an injected interface; Python
needs `freezegun`.

```hd
use std.time.{Clock, Duration, Timestamp, now}

pub fn expired(issued: Timestamp, ttl: Duration) -> bool $ Clock:
    now().since(issued) > ttl

pub fn main() -> void $ Console + Clock:
    println(expired(now(), Duration::seconds(60)))

tests:
    use std.testing.assert_equal
    use std.time.{ManualClock, s}

    it("expires after the ttl"):
        start := Timestamp::from_unix_milliseconds(0)
        let mut clock = ManualClock::new(start)
        $.with(Clock=clock):
            assert_equal(expired(start, s(60)), false, reason="fresh")
            clock.sleep!(s(61))
            assert_equal(expired(start, s(60)), true, reason="stale")
```

## Batteries

Against Go's std, Rust's std with the crates nearly every Rust program
uses (serde, regex, clap, chrono, reqwest, anyhow), and Effect. **80%**:
a common program needs it. **Nice**: some programs do. **Out**: out of
scope, with the reason.

| Gap | Go | Rust | Effect | Class | Note |
| --- | --- | --- | --- | --- | --- |
| Float math: `sqrt`, `floor`, `round`, `abs`, `pow`, `PI` | `math` | `f64` methods | | 80% | `floor`, `round`, `abs` are plain hd; `sqrt` needs an intrinsic (OPEN_ISSUES) |
| Integer `abs`, `pow`, `signum` | `math` | `i32::abs`, `pow` | | 80% | plain hd |
| Directory walk and glob | `filepath` | walkdir, glob | | 80% | change 5 |
| Copy a file; modified time in `Entry` | `io.Copy`, `ModTime` | `fs::copy`, `metadata` | | 80% | mtime is a host-trait field |
| Temporary files and directories | `os.CreateTemp` | tempfile | | nice | `temp_dir()` is only in `std.testing` |
| Line-by-line reads of large files | `bufio.Scanner` | `BufRead` | `Stream` | nice | whole-file reads cover most scripts |
| HTTP status errors and JSON helpers | | reqwest | `HttpClient` | 80% | change 1 |
| URL parsing, building, query encoding | `net/url` | url | | 80% | without it, query strings are concatenated by hand |
| HTTP forms, cookies, multipart | `net/http` | reqwest | | nice | |
| HTTP server | `net/http` | axum | `@effect/platform` | nice | needs a host listener design past `std.net` |
| JSON pointer, `as_i64`, pretty encode | | serde_json | | 80% | change 9 |
| Serde renames, skips, member defaults | struct tags | serde attributes | `Schema` | 80% | open in OPEN_ISSUES; real APIs use camelCase |
| CSV | `encoding/csv` | csv | | 80% | the commonest data file after JSON |
| TOML, YAML | | toml, serde_yaml | | nice | |
| Process working directory, environment, streaming, kill | `os/exec` | `Command` | `Command` | 80% | cwd and env are open in OPEN_ISSUES |
| `run!` and `output!` helpers | `Command.Output` | `Command::output` | | 80% | change 2 |
| Signals, graceful shutdown | `os/signal` | ctrlc | | nice | a host hook |
| Typed CLI structs | typed `flag` | clap derive | `@effect/cli` | nice | open in OPEN_ISSUES |
| Generic `parse::[T]` | `strconv` | `FromStr` | `Schema.decode` | 80% | change 10 |
| `print` without a newline | `fmt.Print` | `print!` | | nice | needs a `Console` method (OPEN_ISSUES) |
| Read all of stdin | `io.ReadAll` | `read_to_string` | | nice | |
| Structured logging | `log/slog` | tracing | `Effect.log` | 80% | waits for the observability hooks |
| Integer radix output, digit grouping | `strconv` | `{:x}` | | 80% | plain hd |
| `rfind`, `splitn`, `rsplit_once` | `strings` | `str` | | 80% | change 6 |
| Regex `(?i)` and `\b` | `regexp` | regex | | 80% | deferred by a stdlib call |
| Unicode normalization | `x/text` | unicode-normalization | | out | large tables; a package |
| `Set` collect, `entries`, `max_by_key`, `dedup`, `most_common` | `slices`, `maps` | itertools | `Array` | 80% | change 6 |
| Sorted map, binary search | `slices.BinarySearch` | `BTreeMap` | `SortedMap` | nice | |
| `Duration::parse`, ns resolution, date format | `time` | chrono, humantime | `Duration` | 80% | change 8 |
| Time zones, local time | `time.LoadLocation` | chrono-tz | `DateTime.Zoned` | nice | needs tz data from the host |
| Date arithmetic, weekday, hour | `time` | chrono | `DateTime` | nice | Hinnant's algorithms are already in `std.time` |
| Timeout combinator | `context` | `tokio::time::timeout` | `Effect.timeout` | 80% | change 7 |
| Bounded concurrency | `errgroup` | `buffer_unordered` | `forEach` `concurrency` | 80% | change 7 |
| Retry predicate, jitter, schedules | | backoff | `Schedule` | nice | the predicate is change 7 |
| Channels, queues, semaphores | `chan`, `sync` | `mpsc` | `Queue` | nice | after the structured `Task` design |
| Streams | | `Stream` | `Stream` | nice | after `Task` |
| Layer-style providers | | | `Layer` | out | requirement rows and `$.with` already do this |
| Caches, once-only values | `sync.Once` | `OnceLock` | `Cache` | nice | the derived-function cache is open |
| HMAC-SHA256 | `crypto/hmac` | hmac | | 80% | webhook signatures; plain hd over `sha256` |
| SHA-1, SHA-512, MD5 | `crypto/*` | sha1, sha2, md5 | | nice | old formats |
| URL-safe base64 | `base64.URLEncoding` | base64 | | 80% | JWTs and tokens; plain hd |
| Secure token helper | `crypto/rand.Text` | | | nice | `token_hex` |
| UUID | package | uuid | | nice | |
| gzip, zip | `compress/*` | flate2 | | nice | |
| Big integers | `math/big` | num-bigint | | out | a package, by STDLIB decision 9 |
| Decimal | | rust_decimal | `BigDecimal` | nice | waits for a `decimal` design |
| Text templates | `text/template` | tera | | out | interpolation covers most uses |
| Benchmarks | `testing.B` | criterion | | nice | |

Counts: 22 gaps for the 80%, 22 nice to have, 4 out of scope.

## Design review

Where an existing API breaks principle 2 or 3, ranked by user value per
cost.

1. **HTTP treats every status as success.** `get!` and `send!`
   (`lib/std/http.hd:105`) return `.Ok` for a 404, so the one-line fetch
   prints an error page and exits 0. Fix: `HttpError.Status(status, url)`
   and `Response.ok()`; `send!` stays raw, and `get_text!` checks.
2. **Host-trait helpers are uneven.** `std.fs` has free helpers for two
   of its ten methods (`lib/std/fs.hd:99`, `:106`; methods at `:80` to
   `:93`), `std.process` for none (`lib/std/process.hd:61`), while
   `std.http`, `std.time` and `std.host` have them. Fix: one free helper
   per method, as Go's `os` package has.
3. **`Process.run!` requires stdin and hides failure in a field**
   (`lib/std/process.hd:61`). Fix: `stdin: string = ""` on the helper,
   and `output!`, with a non-zero exit as `ProcessError.Exited(status,
   stderr)`.
4. **`ProcessError.NotFound` does not name the program**
   (`lib/std/process.hd:53`), while `FsError.NotFound(path)` names its
   path. Fix: `NotFound(program: string)` and the same for
   `PermissionDenied`.
5. **JSON decoding ignores declared defaults.** `port: u16 = 8080`
   decodes a missing key as `MissingField` (`spec/std/json.md:584` lists
   defaults as open), though `Field.default()` exists
   (`lib/std/structure.hd:133`). Fix: default, then `.None` for a `T?`,
   then `MissingField`.
6. **`Rng` has two identical constructors**, `new` and `from_seed`
   (`lib/std/random.hd:78`, `:86`). Pass 89 renamed `from_seed`, but the
   old name stayed public. Fix: make it private or remove it.
7. **`CliError`'s doc promises a help request** (`lib/std/cli.hd:213`)
   that no variant holds, and an undeclared `--help` is an unknown
   option. Fix: correct the doc; later, consider a default `-h/--help`.
8. **`monotonic()` has no free helper**, unlike `now()` and `sleep!`
   (`lib/std/time.hd:163` against `:170`, `:177`). Fix: `monotonic()`
   and `elapsed(start: Instant) -> Duration`, both `$ Clock`.
9. **`Duration` counts milliseconds** (`lib/std/time.hd:20` to `:36`),
   so short work times as `0ms` or `1ms`. Go and Rust count nanoseconds.
   Fix: store nanoseconds. It changes the `Clock` host trait: owner.
10. **`Duration` text says `min` where Go says `m`**
    (`lib/std/time.hd:56`), though the comment cites Go. Fix: a future
    `Duration::parse` reads both.
11. **`Set` cannot be collected:** `FromIterator` covers only `List`,
    `Map`, `Result` and options (`lib/std/iter.hd:244` to `:289`). Fix:
    `impl[T < Eq & Hash] FromIterator[T] for Set[T]`.
12. **JSON numbers take two hops**: `Json.as_number()` then `as_i64()`
    (`lib/std/json.hd:212`). Fix: `Json.as_i64()`, `as_u64()`, `as_f64()`.
13. **Test providers use six naming schemes:** `Scripted*`
    (`lib/std/http.hd:112`, `lib/std/process.hd:67`,
    `lib/std/console.hd:71`), `MemoryFs` (`lib/std/fs.hd:116`),
    `ManualClock` (`lib/std/time.hd:536`), `SeededRandom`
    (`lib/std/random.hd:24`), `Map*` (`lib/std/host.hd:33`, `:54`,
    `lib/std/sys.hd:47`), `BufferConsole` (`lib/std/console.hd:44`).
    Fix: one table in `spec/std/README.md` now, one naming rule later.
14. **`Rng` does not say it is not for secrets** (`lib/std/random.hd:72`).
    Fix: point to `Random.fill` in its doc, and add `token_hex`.
15. **The raw-string prefix `r` is not in the prelude**
    (`lib/std/text.hd:19`), so every regex program imports it. The
    prelude is language tier: owner question.
16. **`it_each` and `it_prop` take no trailing block**
    (`lib/std/testing.hd:509`, `:514`), unlike `it`. Language tier: owner
    question.

## Method and limits

- Go and Python counts are estimates: read them for which calls exist.
- Tasks 6 and 28 were checked, not run; they need a remote server.
- Of the seven programs that failed their first check, six failed on my
  syntax (lambda form, `match` as an operand, a discarded `pop()`, a
  private type in `main`, `let mut`, `it_each`'s named body). The seventh
  was the missing `use std.text.r` (design review 15).
