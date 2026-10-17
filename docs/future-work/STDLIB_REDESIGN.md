# Standard Library Redesign

This is a proposal, not a decision. It reviews the public surface of
`lib/std` against the owner's bar for #350, after the owner rejected the
first audit's shortcuts (`get_text!`, `post_json!`, `process.output!`) as
neither composable nor small. Job 1 covers I/O and the host; jobs 2 and 3
add their groups below, following the same convention.

## The Bar And The Shared Convention

The bar is the owner's seven principles for #350. The library has the
batteries a common program needs (1). It follows the designs that Go,
Effect and Rust prove (2). It uses hd's own features: capabilities in
requirement rows, `!` only where a call can suspend, `Result` and `?`,
derive, and providers swapped in tests (3). The common 80% is ergonomic,
judged by realistic programs, not one-liners (4). It is built from
composable atoms, never a function that fixes one combination of options
(5). Its designs are stress-tested for correctness (6). Its built-ins
are fast, with no naive algorithm and no cost that an implementation
cannot remove later (7). Correctness, ergonomics and usability come first
in the order of work; tuning waits, but no design here forces a cost.

Every module in this document follows one shape:

- **Builder.** A plain data value that does no I/O. It starts from a
  constructor, `Type::new(...)`, or a named constructor such as
  `Request::get(url)`. Its setters are bare nouns (`header`, `arg`,
  `timeout`), take `mut self`, change the value in place, and return it,
  so calls chain. There is no `with_` or `set_` prefix on a builder.
  `with_x` names a copy of an immutable value with one part replaced
  (`Path.with_extension`). `set_x` is only for a live handle's setting
  (`TcpStream.set_timeout`).
- **Terminal.** One bang method per builder does the I/O and names the
  capability in its row (`Request.send!`, `Command.run!`). An outcome that
  is not success, such as an HTTP 404 or an exit status of 1, is an
  `.Err` that carries the whole result, so a caller that treats it as data
  matches it and reads it.
- **Accessors.** Stored data is a public field (`response.status`). A
  method exists only when it computes (`response.text()`). `to_x` builds
  a new value, `as_x` reads one cheaply, and `Type::parse(text)` reads
  text. hd has no `into_x`.
- **Free functions.** One per host-trait method that starts work, named
  as the method (`read_text!`, `connect!`), so a program need not write
  `$.use`. None fixes a combination of options.
- **Errors.** One enum per module, `XError`. Variants name their subject
  (`NotFound(path)`), and every module has `NotGranted(subject)` and
  `Other(message)`. Each implements `Eq`, `Debug`, `Display` and `Error`.
- **Test providers.** These keep the four prefixes `Scripted`, `Memory`,
  `Manual` and `Seeded`, beside their trait. A `Scripted` provider that
  answers requests is built empty with `new()`. `reply(key, result)`
  queues an answer per key, and the last one repeats, as Python's
  `responses` library serves them. `received()` lists what it was asked,
  in order.
- **Cost.** A setter copies nothing and appends in O(1) amortized, as
  reqwest's `RequestBuilder` and Rust's `Command` do. An accessor copies
  the payload at most once, and only to convert it (bytes to text).
  Strings stay Go's: immutable UTF-8, built with `StringBuilder`. A
  design may buffer a whole body only where a streaming terminal can
  later sit beside it with no change to the buffered one.

Each module section ends with one **Cost** line. Names are counted in the
last section.

## std.http

**Verdict: fails the bar.** `get!`, `get_text!` and `post_json!` each fix
one combination. There is no query encoding, no auth, no cookies, no
forms and no client with defaults, so a realistic call builds a `Request`
literal and its headers by hand. The status check is opt-in through
`ok()`, which is why the one-line fetch needed its own helper.

The proposal follows reqwest's `RequestBuilder` and `Client`, Go's
`http.Client` with its `Jar` and `client.Do(req)`, and ky's status error,
which carries the response.

```text
pub data Request:
    pub method: Method = Method.Get
    pub url: string
    pub query: mut List[(string, string)] = []
    pub headers: mut List[(string, string)] = []
    pub body: List[u8] = []
    pub timeout: Duration? = .None
    pub redirects: usize = 10

impl Request:
    pub fn get(url: string) -> mut Request
    pub fn post(url: string) -> mut Request
    pub fn new(method: Method, url: string) -> mut Request
    pub fn header(mut self, name: string, value: string) -> mut Request
    pub fn query(mut self, name: string, value: string) -> mut Request
    pub fn body(mut self, bytes: List[u8]) -> mut Request
    pub fn json[T < Serialize](mut self, value: T) -> mut Request
    pub fn form(mut self, fields: List[(string, string)]) -> mut Request
    pub fn timeout(mut self, limit: Duration) -> mut Request
    pub fn redirects(mut self, max: usize) -> mut Request
    pub fn send!(self) -> Result[Response, HttpError] $ Http

impl Response:                                # fields as today; headers and body default to []
    pub fn header(self, name: string) -> string?
    pub fn text(self) -> string
    pub fn json[T < Deserialize](self) -> Result[T, HttpError]

pub data Client: ...                         # private fields: defaults and the cookie jar

impl Client:
    pub fn new() -> mut Client
    pub fn base_url(mut self, url: string) -> mut Client
    pub fn header(mut self, name: string, value: string) -> mut Client
    pub fn timeout(mut self, limit: Duration) -> mut Client
    pub fn send!(mut self, request: Request) -> Result[Response, HttpError] $ Http
    pub fn cookie(self, name: string) -> string?

impl HttpError:
    pub fn is_transient(self) -> bool         # Timeout, Connect, or Status 408, 429, 500, 502, 503, 504

impl ScriptedHttp:
    pub fn new() -> mut ScriptedHttp
    pub fn reply(mut self, url: string, result: Result[Response, HttpError]) -> void
    pub fn received(self) -> List[Request]
```

- `send!` sends through the covering `Http` and returns
  `.Err(HttpError.Status(response))` for a status outside 200 to 299. The
  variant changes from `Status(status, url)` to `Status(response:
  Response)`, so an API's error body stays readable. The `Http` trait is
  unchanged: one raw, status-neutral round trip.
- `query` pairs are percent-encoded (RFC 3986 unreserved characters kept)
  and added to the URL once, at send time. `json` encodes the body and
  sets `Content-Type: application/json` unless a header names it. `form`
  writes `application/x-www-form-urlencoded`.
- `Client.send!` resolves a URL that starts with `/` against `base_url`.
  It adds each default header that the request does not name, and the
  default timeout when the request has none. Default headers go to every
  host the client calls, as reqwest's `default_headers` do.
- The client keeps cookies, as Python's `requests.Session` does. To keep
  a cookie set during a redirect, the client follows redirects itself, as
  Go's `Client` does over its `Transport`. It asks the provider for
  `redirects: 0`, then follows up to the request's limit. A 303, and a
  301 or 302 after a `Post`, becomes a `Get`.
- Security: cookies are host-only (the `Domain` attribute is ignored,
  since std has no public-suffix list), a `Secure` cookie goes only over
  `https`, and `Max-Age=0` deletes. A redirect to another host drops
  `Authorization` and `Cookie`, as Go and reqwest do.
- Basic and bearer auth are one `header` call each; no helper.
- Removed: `get!`, `get_text!`, `post_json!`, the free `send!`, and
  `Response.ok()`.
- Host change: `Request.redirects` reaches the provider. Status checks,
  query encoding, forms, JSON and cookies are plain hd.

**Cost.** Each setter appends to one list in place. The query is encoded
once, into one `StringBuilder`. `json` encodes once; the copy from text
to bytes stays hidden behind the setter until a bytes view exists.
`text()` checks and copies the body once per call, as Go's
`string(body)` does, and `json::[T]()` decodes from that text with
zero-copy strings. The whole body is buffered, so large downloads wait
for the streaming terminal in the owner questions.

## std.net

**Verdict: meets the bar, with two fixes.** Sockets are handles, as in
Rust's `std::net` and Go's `net.Conn`, and `set_` is used only on live
handles. `connect_timeout!` fixes connect plus a timeout, and
`std.task.timeout!` already composes them, as `tokio::time::timeout`
does: a cancelled connect leaves no socket, and the work wins a tie, so a
scripted connect beats a `ManualClock`. `Net` is also the one host trait
with no free helpers.

```text
pub fn lookup!(host: string) -> Result[List[string], NetError] $ Net
pub fn connect!(host: string, port: u16) -> Result[mut dyn TcpStream, NetError] $ Net
pub fn listen!(host: string, port: u16) -> Result[mut dyn TcpListener, NetError] $ Net
pub fn bind_udp!(host: string, port: u16) -> Result[mut dyn UdpSocket, NetError] $ Net

attempt := timeout!(3s):              # replaces connect_timeout!(host, 25, 3s)
    connect!(host, 25)
```

- Removed: `Net.connect_timeout!` and `ScriptedNet.connect_timeouts()`,
  which undoes the owner-ordered #259 edit (question 4).
- `ScriptedNet.requested()` becomes `received()`, the shared name.
- `set_timeout` stays on handles. A read loop with `timeout!` around each
  `read!` would pay one race and one timer per read; Rust and Go keep the
  setting for the same reason.

**Cost.** `read!(max)` returns a new list on every call. A later
`read_into!(buffer)` beside it can remove that allocation without
changing `read!`.

## std.fs

**Verdict: meets the bar.** Each free helper is one host operation with
no options, as Go's `os.ReadFile` and Python's `pathlib` methods are.
`walk!` and `glob!` are atoms. Two additions: `FsWrite.copy!(from, to)`
with its free `copy!`, and the field `Entry.modified: Timestamp? = .None`.

- Removed: `exists!`. It is `stat!(path)?.is_some()`, and `stat!` already
  says missing with `.None`.
- `copy!` is a host operation, as Rust's `fs::copy` and Go's `io.Copy`
  are, because copying through hd crosses the host twice and copies the
  bytes twice. `modified` serves "rebuild what changed", the commonest
  reason to stat a file.
- Not proposed: temporary directories, and `OpenOptions`-style flags.
  Reading a large file line by line is the streaming question.

**Cost.** `read_text!` takes its string from the host with one copy.
`walk!` lists each directory once. Today it builds two strings per sort
comparison, and `MemoryFs` scans every file per call, both filed for
#426.

## std.path

**Verdict: meets the bar.** `Path` is a pure value. Two additions serve
the common "write the output beside the input" step, as Rust's `Path`
and `pathlib`'s `stem` and `with_suffix` do:

```text
impl Path:
    pub fn file_stem(self) -> string?                  # "a.tar.gz" gives "a.tar"
    pub fn with_extension(self, extension: string) -> Path
```

**Cost.** Each method copies the text once. `join` concatenates twice
today, which one concatenation fixes.

## std.process

**Verdict: fails the bar.** `Process.run!(program, args, stdin)` is
positional, has no environment or working directory, and is wrapped by
two helpers that fix combinations. `run!` reports a non-zero exit as
data and `output!` as an error, which is two shapes for one fact.
`ScriptedProcess` ignores the arguments, so a test cannot check them.

The proposal is Rust's `Command`, with the error shape of Go's
`Cmd.Output` (an `ExitError` that carries stderr) and of Python's
`run(check=True)`:

```text
pub data Command:
    pub program: string
    pub args: mut List[string] = []
    pub env: mut List[(string, string)] = []
    pub current_dir: Path? = .None
    pub stdin: string = ""

impl Command:
    pub fn new(program: string) -> mut Command
    pub fn arg(mut self, value: string) -> mut Command
    pub fn args(mut self, values: List[string]) -> mut Command
    pub fn env(mut self, name: string, value: string) -> mut Command
    pub fn current_dir(mut self, dir: Path) -> mut Command
    pub fn stdin(mut self, text: string) -> mut Command
    pub fn run!(self) -> Result[ProcessOutput, ProcessError] $ Process

pub enum ProcessError:
    ...                                        # NotFound, PermissionDenied, NotGranted, Other as today
    Exited(output: ProcessOutput)              # was Exited(status, stderr)

pub trait Process:                             # language tier: owner
    fn run!(mut self, command: Command) -> Result[ProcessOutput, ProcessError]

impl ScriptedProcess:
    pub fn new() -> mut ScriptedProcess
    pub fn reply(mut self, program: string, result: Result[ProcessOutput, ProcessError]) -> void
    pub fn received(self) -> List[Command]
```

- `run!` returns `.Err(ProcessError.Exited(output))` for a non-zero
  status, with stdout and stderr kept. `ProcessOutput`'s fields default to
  `""`, `""` and 0, so a test writes only what matters.
- A timeout is `timeout!(limit): command.run!()`. Cancelling `run!`
  already kills the child's process group.
- Removed: the free `run!` and `output!`.
- The trait change is language tier and changes the host ABI
  (question 3).

**Cost.** `arg` and `env` append in place. The host returns stdout as a
string that std passes on without a copy. Output is buffered whole, so a
long-running or chatty child waits for the streaming question.

## std.console

**Verdict: meets the bar, with two gaps.** `println`, `eprintln` and
`read_line!` are right. A filter cannot read all of stdin in one call,
and a prompt cannot print without a newline.

```text
pub trait ConsoleInput:
    fn read_line!(mut self) -> Result[string?, ConsoleError]
    fn read_all!(mut self) -> Result[string, ConsoleError]: ...   # default body joins read_line!

pub fn read_all!() -> Result[string, ConsoleError] $ ConsoleInput

pub trait Console:                                                 # language tier: owner
    fn write!(mut self, text: string) -> Result[void, ConsoleError]
    ...
pub fn print[T < Display](value: T) -> void $ Console
```

The default body means no provider has to change. The host overrides
`read_all!` with one read, as Go's `io.ReadAll(os.Stdin)` does.

**Cost.** The host's `read_all!` copies the input once; the default body
joins lines with one `StringBuilder`. `println` builds `"$value"` even
when the value is already a string.

## std.host and std.sys

**Verdict: `std.host` meets the bar; `std.sys` is a module for four
reads.** `args()`, `env(name)` and `env_as::[T](name)` are plain calls,
and `env_as` composes the `Parse` trait. Proposal: move `Sys`,
`SysError` and `ScriptedSys` into `std.host`, as Go keeps `os.Hostname`
beside `os.Getenv` and Rust keeps `env::consts::OS` in `std::env`. No
name changes, and `Sys` keeps no free helpers, since its reads are rare.

**Cost.** Each read is one host call with no copy in std.

## std.cli

**Verdict: the shape is right; the builder and `Parsed` are not.** Each
declaration copies both lists, so declaring n options costs O(n²).
`Parsed` offers public fields and accessors for the same data, and a
repeated option keeps only its last value, so `-x a -x b` loses `a`.

```text
impl Cli:                       # new, flag, option, positional now take and return mut
    pub fn flag(mut self, name: string, short: char?, help: string) -> mut Cli
    ...                         # parse, parse_args, usage unchanged

impl Parsed:                    # fields become private; flag, value, value_as unchanged
    pub fn values(self, name: string) -> List[string]      # new: every value, in order
    pub fn positionals(self) -> List[string]               # new
```

Subcommands need no new name: the program reads the first argument and
parses the rest with a second `Cli`, as Go's `flag.NewFlagSet` does
(scenario 4).

**Cost.** Setters append in place. `parse` finds an option through a
map instead of a scan, and `usage` writes one `StringBuilder`.

## std.time

**Verdict: meets the bar.** Time values need no builder. The cuts are
duplicates:

- `Duration::milliseconds(n)` and `Duration::seconds(n)` are removed. The
  suffix functions `ms(n)` and `s(n)` are the same calls, and `5s` is
  the literal.
- `ParseDurationError` merges into `TimeParseError`: both are a variant
  and a byte position, and one parse error type serves all time text.

**Cost.** `Duration`, `Timestamp` and `Instant` are one-field data over an
`i64`. As newtypes over `i64` they need no heap object, with no API
change. `to_rfc3339` and `format` build text with `+` chains today,
filed for #426.

## Scenarios

Each program uses only the proposed surface. Each parses with
`scripts/hd-conformance.sh parse`; none checks today. The module
sketches above are signature lists, not programs.

**1. A paginated API client** with a token from the environment, a query,
retries on transient failures, and context on errors. Its test answers
raw responses, so it runs the real status check and the real retry.

```text
use std.http.{Client, Http, HttpError, Request}
use std.host.{Env, env}
use std.serde.Deserialize
use std.task.{Backoff, retry_with}
use std.time.{Clock, ms, s}
use std.error.Error

@derive(Deserialize)
data Issue:
    number: u32
    title: string

fn transient(error: HttpError) -> bool:
    error.is_transient()

fn github(token: string) -> mut Client:
    let mut api = Client::new().base_url("https://api.github.com").timeout(10s)
    api.header("Authorization", "Bearer $token")
    api.header("Accept", "application/vnd.github+json")
    api

fn open_issues!(api: mut Client, repo: string) -> Result[List[Issue], HttpError] $ Http + Clock:
    policy := Backoff { attempts: 4, initial: 200ms, factor: 2, max: 5s }
    let found: mut List[Issue] = []
    let page: u32 = 1
    while true:
        request := Request::get("/repos/$repo/issues").query("state", "open").query("page", "$page")
        result := retry_with!(policy, when=transient):
            api.send!(request)
        batch := result?.json::[List[Issue]]()?
        if batch.is_empty():
            break
        found.extend(batch)
        page = page + 1
    .Ok(found)

pub fn main!() -> Result[void, dyn Error] $ Console + Env + Http + Clock:
    token := env("GITHUB_TOKEN").context("GITHUB_TOKEN is not set")?
    let mut api = github(token)
    for issue in open_issues!(api, "hd-lang/hd")?:
        println("#${issue.number} ${issue.title}")
    .Ok(())

tests:
    use std.http.{Response, ScriptedHttp}
    use std.time.{ManualClock, Timestamp}
    use std.testing.assert_equal

    it("retries a 503 and stops at an empty page"):
        page1 := "https://api.github.com/repos/a/b/issues?state=open&page=1"
        page2 := "https://api.github.com/repos/a/b/issues?state=open&page=2"
        let mut http = ScriptedHttp::new()
        http.reply(page1, .Ok(Response { status: 503 }))
        http.reply(page1, .Ok(Response { status: 200, body: "[{\"number\":7,\"title\":\"crash\"}]".to_utf8() }))
        http.reply(page2, .Ok(Response { status: 200, body: "[]".to_utf8() }))
        let mut clock = ManualClock::new(Timestamp::from_unix_milliseconds(0))
        $.with(Http=http, Clock=clock):
            let mut api = github("t0k")
            match open_issues!(api, "a/b"):
                .Ok(issues) => assert_equal(issues.len(), 1, reason="one issue")
                .Err(error) => panic("unexpected $error")
        sent := http.received()
        assert_equal(sent.len(), 3, reason="a retry, then page 2")
        assert_equal(sent[0].headers.contains(("Authorization", "Bearer t0k")), true, reason="the token")
```

**2. A form login with a session cookie and a CSRF token**, then an
authenticated JSON update; a wrong password is a status the program
reads as data.

```text
use std.http.{Client, Http, Request}
use std.serde.Serialize
use std.time.s
use std.error.{Error, message}

@derive(Serialize)
data Profile:
    display_name: string

fn rename!(user: string, password: string, name: string) -> Result[void, dyn Error] $ Http:
    let mut site = Client::new().base_url("https://shop.example").timeout(15s)
    _ := site.send!(Request::get("/login"))?
    csrf := site.cookie("csrftoken").context("the login page set no CSRF cookie")?
    login := Request::post("/login").form([("username", user), ("password", password), ("csrfmiddlewaretoken", csrf)])
    match site.send!(login):
        .Ok(_) => pass
        .Err(.Status(response)) if response.status == 401 => return .Err(message("wrong password for $user"))
        .Err(error) => return .Err(error)
    update := Request::new(.Put, "/api/profile").header("X-CSRFToken", csrf).json(Profile { display_name: name })
    _ := site.send!(update)?
    .Ok(())
```

**3. A release script** that runs tools in a directory with an
environment, reads an exit status of 1 as data, bounds a build, and
feeds stdin; then its test.

```text
use std.process.{Command, Process, ProcessError}
use std.path.Path
use std.task.timeout
use std.time.{Clock, min}
use std.error.{Error, message}

fn has_changes!(repo: Path) -> Result[bool, ProcessError] $ Process:
    match Command::new("git").args(["diff", "--quiet"]).current_dir(repo).run!():
        .Ok(_) => .Ok(false)
        .Err(.Exited(output)) if output.status == 1 => .Ok(true)
        .Err(error) => .Err(error)

pub fn main!() -> Result[void, dyn Error] $ Console + Process + Clock:
    repo := Path("crates/app")
    if has_changes!(repo)?:
        return .Err(message("commit or stash your changes first"))
    let mut build = Command::new("cargo").args(["build", "--release"]).current_dir(repo)
    build.env("CARGO_TERM_COLOR", "never")
    result := timeout!(10min):
        build.run!()
    _ := result?.context("cargo build")?
    tag := Command::new("git").args(["describe", "--tags"]).current_dir(repo).run!()?.stdout.trim()
    signed := Command::new("gpg").arg("--clearsign").stdin("release $tag\n").run!()?.stdout
    println(signed)
    .Ok(())

tests:
    use std.process.{ProcessOutput, ScriptedProcess}
    use std.testing.assert_equal

    it("reads exit 1 from git diff as changes"):
        let mut process = ScriptedProcess::new()
        process.reply("git", .Ok(ProcessOutput { status: 1 }))
        $.with(Process=process):
            assert_equal(has_changes!(Path("crates/app")), .Ok(true), reason="exit 1 means changes")
        ran := process.received()
        assert_equal(ran[0].args, ["diff", "--quiet"], reason="the arguments")
        assert_equal(ran[0].current_dir, .Some(Path("crates/app")), reason="the directory")
```

**4. A file converter** with a subcommand, a repeated option, a glob,
and output paths derived from input paths.

```text
use std.cli.Cli
use std.fs.{FsRead, FsWrite, create_dir_all, glob, read_text, write_text}
use std.host.{Args, args}
use std.path.Path
use std.error.{Error, message}

fn build!(arguments: List[string]) -> Result[usize, dyn Error] $ Console + FsRead + FsWrite:
    let mut cli = Cli::new("notes build").flag("help", .Some('h'), "show this help")
    cli.option("exclude", .Some('x'), "skip paths that contain this text; repeatable")
    cli.option("out", .Some('o'), "the output directory, site by default")
    cli.positional("pattern", "the files to convert, such as docs/**/*.md")
    parsed := cli.parse(arguments)?
    if parsed.flag("help"):
        println(cli.usage())
        return .Ok(0)
    out := Path(parsed.value("out").unwrap_or("site"))
    create_dir_all!(out)?
    excluded := parsed.values("exclude")
    let count: usize = 0
    for source in glob!(parsed.positionals()[0])?:
        if excluded.iter().any(fn(text: string) -> bool: "$source".contains(text)):
            continue
        name := source.with_extension("html").file_name().unwrap_or("index.html")
        write_text!(out.join(name), "<pre>${read_text!(source)?}</pre>")?
        count = count + 1
    .Ok(count)

pub fn main!() -> Result[void, dyn Error] $ Console + Args + FsRead + FsWrite:
    all := args()
    let rest: List[string] = all.iter().skip(1).collect()
    if all.get(0) == .Some("build"):
        count := build!(rest)?
        println("converted $count files")
        return .Ok(())
    .Err(message("usage: notes build [options] <pattern>"))
```

**5. A pipe filter** that reads all of stdin, as `cat orders.json | total`.

```text
use std.console.{ConsoleInput, read_all}
use std.json.decode
use std.serde.Deserialize
use std.error.Error

@derive(Deserialize)
data Order:
    id: string
    cents: i64

pub fn main!() -> Result[void, dyn Error] $ Console + ConsoleInput:
    orders := decode::[List[Order]](read_all!()?)?
    total := orders.iter().map(fn(order: Order) -> i64: order.cents).sum()
    println("${orders.len()} orders, $total cents")
    .Ok(())
```

## Public Names

A name is a public type, trait, or function, or a public method of one,
trait methods included. Fields and variants are not counted. "Before" is
`lib/std` at `5be3959c7`.

| Module | Before | After | Removed | Added |
| --- | --- | --- | --- | --- |
| `std.http` | 17 | 32 | `get!`, `get_text!`, `post_json!`, free `send!`, `ok`, `sent` | `Client` and its 6 methods; `Request::get`, `post`, `new`, `header`, `query`, `body`, `json`, `form`, `timeout`, `redirects`, `send!`; `is_transient`; `reply`, `received` |
| `std.net` | 56 | 58 | `connect_timeout!`, `connect_timeouts`, `requested` | free `lookup!`, `connect!`, `listen!`, `bind_udp!`; `received` |
| `std.fs` | 30 | 31 | `exists!` | `FsWrite.copy!`, free `copy!` |
| `std.path` | 5 | 7 | none | `file_stem`, `with_extension` |
| `std.process` | 11 | 19 | free `run!`, `output!` | `Command` and its 7 methods; `reply`, `received` |
| `std.console` | 15 | 19 | none | `read_all!` (method and free), `write!`, `print` |
| `std.host` with `std.sys` | 22 | 22 | the module `std.sys` | none |
| `std.cli` | 13 | 15 | none | `values`, `positionals` |
| `std.time` | 35 | 32 | `Duration::milliseconds`, `seconds`, `ParseDurationError` | none |
| **Total** | **204** | **235** | **15** | **46** |

The growth is in HTTP and processes, where today's surface could not
express auth, cookies, queries, forms, a working directory or an
environment. Most added names appear in a scenario above; the rest
(`body`, `redirects`, the free net helpers, `copy!`, `file_stem`,
`print`) each close a gap the bar names. Each removed name is a fixed
combination or a duplicate.

## Questions For The Owner

1. **Constructor spelling.** `Request::get(url)` keeps one rule for every
   builder (`Command::new`, `Cli::new`, `Client::new`), while free
   `get(url)` and `post(url)` read like ky and `reqwest::get`.
   Recommendation: `Request::get`, `Request::post`, `Request::new`.
2. **Status as an error by default.** ky, Python's `urllib`, and Go's
   `Cmd.Output` fail a non-success outcome by default; reqwest and
   `fetch` make it opt-in. Recommendation: by default, in `send!` and
   `run!`, with the whole response or output in the error.
3. **Language-tier trait changes.** `Process.run!` takes a `Command`
   (environment, working directory), and `Console` gains `write!` for
   `print`. Both change the host ABI. Recommendation: approve both.
4. **Undo `connect_timeout!`.** It was ordered on 2026-10-16 (#259);
   `timeout!` around `connect!` does the same with no extra trait method.
   Recommendation: remove it.
5. **Streaming.** HTTP bodies, process output, and large files are
   buffered whole. Recommendation: one reader handle shape for all three,
   the `read!(max)` of `TcpStream`, added later as a second terminal
   (`send_streaming!`, `spawn!`, `open!`) beside the buffered ones, which
   stay unchanged.
