# Standard Library Redesign

This is a proposal, not a decision. It reviews the public surface of
`lib/std` against the owner's bar for #350, after the owner rejected the
first audit's shortcuts (`get_text!`, `post_json!`, `process.output!`) as
neither composable nor small. Job 1 covers I/O and the host; jobs 2 and 3
add their groups below, following the same convention. The I/O sections
were revised against principle 8 after the owner rejected the first
`std.cli` as untyped: "it is impossible to parse the result into a data
type, so everything is checked."

## The Bar And The Shared Convention

The bar is the owner's eight principles for #350. The library has the
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

The owner added an eighth principle on 2026-10-17: "8. Strongly typed
interfaces by default; untyped APIs may provide an escape hatch". It
mirrors principles 4 and 5. The default path returns typed values,
reads text into the program's own types through derive, and makes
invalid states unrepresentable: enums for closed sets, required against
defaulted fields, newtypes for units and IDs. An untyped, string-keyed
layer, such as a dynamic `Json` or regex groups read by name, stays
public as the escape hatch, and the typed layer is built on it.

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
- **Typed by default.** Data that comes from outside the program reads
  into the program's own type: `response.json::[T]()`,
  `Options::parse_args()`, `env_as::[T](name)`. A value going out is
  taken as a typed value through `Serialize`: `request.json(value)`,
  `request.form(value)`. The string-keyed form stays public beneath the
  typed one, for shapes known only at run time: `Parsed.value(name)`,
  `response.header(name)`, `env(name)`.
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
    pub fn query[T < Display](mut self, name: string, value: T) -> mut Request
    pub fn body(mut self, bytes: List[u8]) -> mut Request
    pub fn json[T < Serialize](mut self, value: T) -> mut Request
    pub fn form[T < Serialize](mut self, value: T) -> mut Request
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
    pub fn cookie(self, url: string, name: string) -> string?

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
  and added to the URL once, at send time. The value is any `Display`, so
  a page number needs no `"$page"`. `json` encodes the body and sets
  `Content-Type: application/json` unless a header names it.
- `form` writes `application/x-www-form-urlencoded` from a `Serialize`
  value, as reqwest's `form(&T)` does through serde. Each member is one
  field. `.None` is left out, and a list repeats the name, as Go's
  `url.Values` does. A `Map[string, string]` literal is the untyped form.
  A member that is a nested data type or map panics, since the type is
  fixed in source and the first run finds it.
- `Client.send!` resolves a URL that starts with `/` against `base_url`.
  It adds each default header that the request does not name, and the
  default timeout when the request has none. Default headers go to every
  host the client calls, as reqwest's `default_headers` do.
- The client keeps cookies, as Python's `requests.Session` does. To keep
  a cookie set during a redirect, the client follows redirects itself, as
  Go's `Client` does over its `Transport`. It asks the provider for
  `redirects: 0`, then follows up to the request's limit. A 303, and a
  301 or 302 after a `Post`, becomes a `Get`. `redirects(0)` follows
  none, as reqwest's `Policy::limited(0)` does.
- `cookie(url, name)` reads the cookie that the client would send to
  `url`, resolved against `base_url`, as Go's `Jar.Cookies(url)` does. A
  name alone does not pick a cookie once two hosts each set one.
- Security: cookies are host-only (the `Domain` attribute is ignored,
  since std has no public-suffix list), a `Secure` cookie goes only over
  `https`, and `Max-Age=0` deletes. A redirect to another host drops
  `Authorization` and `Cookie`, as Go and reqwest do.
- Basic and bearer auth are one `header` call each; no helper.
- Typed by default: bodies go out through `json` and `form` and come back
  through `json::[T]()`. `Method` is a closed enum whose `Other(name)` is
  the escape hatch. The status stays a plain `u16`, as the owner ruled
  for `StatusCode`. Header names stay strings, compared without ASCII
  case, as Go, reqwest and `fetch` accept them; a set of name constants
  would add names and remove no invalid state. The URL stays text, and
  `send!` reports `InvalidUrl`, as reqwest's `get(&str)` and `fetch` do.
- Removed: `get!`, `get_text!`, `post_json!`, the free `send!`, and
  `Response.ok()`.
- Host change: `Request.redirects` reaches the provider. Status checks,
  query encoding, forms, JSON and cookies are plain hd.

**Cost.** Each setter appends to one list in place. The query is encoded
once, into one `StringBuilder`. `form` is one walk of the value into one
`StringBuilder`. `json` encodes once; the copy from text
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
- Addresses stay a `host: string` and a `port: u16`, as Rust's
  `connect((host, port))` takes them. The `u16` already rules out a bad
  port. A typed `SocketAddr` is not proposed: Rust's holds only an IP, so
  a host name would still need the pair, and Go passes `"host:port"`
  text. `lookup!` returns IP text, as Go's `LookupHost` does, because
  every consumer is a call that takes a host string.

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
- Typed already: `Entry.kind` is the closed `EntryKind` enum, `size` is
  bytes as a `u64`, and `modified` is a `Timestamp`. A typed file is
  `decode::[T](read_text!(path)?)`; no `read_json!` fixes that pair.

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

impl Parse for Path:                                   # type Error = never
```

`Path` implements `Parse`, which never fails, so a path is a typed
command-line field, an `env_as::[Path]` read, or a regex group with no
conversion by hand. An impl adds no name.

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
- Arguments as `List[string]` and the environment as name and value
  pairs are Rust's `Command` and Go's `Cmd`. Each argument is passed as
  one item with no shell, so there is no quoting to get wrong. `status`
  stays an `i32`, with 128 plus the signal number for a signal, as the
  spec and the shell already have it.
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
`read_all!` with one read, as Go's `io.ReadAll(os.Stdin)` does. A typed
read composes: `read_line!()?` then `parse::[T]()`, or `decode::[T]` over
`read_all!()?`.

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

`env_as::[T]` is the typed read of one variable. A service configured
only from the environment derives `Cli` and marks each field with
`@from_env`, as clap's `env` setting serves both. That needs no second
derive. `args()` stays the untyped list beneath `parse_args`.

**Cost.** Each read is one host call with no copy in std.

## std.cli

**Verdict: fails the bar.** It breaks principle 8. A program declares its
arguments by name and reads them back by name as text.
`parsed.value("out")` is a `string?`, so a misspelled name is a silent
`.None`, a required option looks like an optional one, and every number
is parsed and checked by hand. Nothing turns the result into the
program's own type. The builder also copies both lists per declaration,
O(n²) for n options, and a repeated option keeps only its last value.

The proposal is clap's derive, which argh's `FromArgs`, Swift's
ArgumentParser and Python's typer share: a data type describes the
command line, and parsing returns a value of that type or an error. The
derive is library code over typed derivation, as `Serialize` is. The
string-keyed builder stays public as the escape hatch for command lines
defined at run time, and the derive is built on it, as clap's derive
sits on its `Command` builder.

### The Typed Layer

```text
## Pack files into a release archive.
@derive(Cli)
data Pack:
    ## Print each file as it is added.
    @short('v')
    verbose: bool
    ## The compression level.
    level: u8 = 6
    ## Skip paths that contain this text.
    @short('x')
    exclude: List[string]
    ## The archive to write.
    @positional
    archive: Path

pack := Pack::parse_args()?          # a Pack, or a CliError
```

Each field's type decides how it reads:

| Field | On the command line | When absent |
| --- | --- | --- |
| `verbose: bool` | the flag `--verbose` | `false` |
| `level: u8 = 6` | the option `--level <level>`, read by `u8`'s `Parse` | the default |
| `name: string`, no default | a required option | `MissingArgument("--name")` |
| `exclude: List[string]` | a repeatable option; every value, in order | `[]` |
| `limit: u32?` | an option | `.None` |
| `format: Format`, an enum with no payloads | an option whose values are the variant names, listed in the help | the default, or required |
| `command: Command`, an enum whose variants carry data | subcommands: the first non-option argument names a variant, and the rest is read by its payload | required, unless `Command?` |
| `@positional archive: Path` | a positional, in field order; a `List[T]` positional takes the rest | as the rows above |

- Names are kebab case: the field `dry_run` is `--dry-run`, and the
  variant `DryRun` is `dry-run`.
- A field's `##` doc comment is its help. The type's doc comment is the
  about line, and a variant's is its command's or value's help.
- `-h` and `--help` are always declared. Either one stops parsing with
  `CliError.Help(usage)`, which holds the help text of the command it
  appeared under.
- Three facts configure a field: `@short(letter)`, `@from_env(name)` and
  `@positional`. Each is a typed fact over `F < Cli`, so a fact on a
  field the derive cannot read is a compile error.
- Every field type must implement `Cli`, which the derived bounds check
  at the `@derive` line. std implements it for `bool`, `string`, `char`,
  the integers and floats, `Path`, `Duration`, and for `T?` and
  `List[T]` over any `T < Cli`. An enum derives it, and a newtype derives
  it through its base type, as in `@derive(Cli) type Port(u16)`. Any
  other type with `Parse` implements it in two short methods (scenario
  6).

Invalid states do not reach the program. A parsed value has every
required field, each of its type. A value outside an enum is an
`InvalidValue` that lists the choices. A subcommand's options exist only
inside its variant, so `notes clean -x tmp` is an `UnknownOption`, not an
ignored flag.

```text
pub trait Cli:
    fn declare(parser: mut ArgParser, arg: Arg) -> void
    fn read(parsed: Parsed, arg: Arg) -> Result[Self, CliError]
    fn parser() -> mut ArgParser: ...                              # the type's declaration, built once per call
    fn parse(arguments: List[string]) -> Result[Self, CliError]: ...   # parser().parse, then read
    fn parse_args() -> Result[Self, CliError] $ Args + Env: ...        # the host's arguments and @from_env variables

@annotate::[F](.Field)
pub data Short[F < Cli]:
    pub letter: char
pub fn short[F < Cli](letter: char) -> Short[F]
...                                       # FromEnv with from_env(name), Positional with positional()
```

`parse` is the pure call that tests use. `parse_args` takes the program
name from `Args` and reads only the variables that `@from_env` facts
name, never `Env.names()`. Its row names `Env` even for a type with no
`@from_env` field, because the row is fixed per call, not per type.

### How The Derive Is Built

The template is plain hd over `std.structure`, in the shape of
`Serialize` and `Deserialize`. Its `declare` describes the type into the
untyped builder, and its `read` builds the value from `Parsed`:

```text
impl[T] Cli for T by Structure:
    fn declare(parser: mut ArgParser, arg: Arg) -> void:
        let mut describer = CliDescriber::[T] { parser: parser, arg: arg, commands: [], choices: [] }
        _ := T::describe(describer)
        describer.finish()        # a value set becomes one Arg with choices

    fn read(parsed: Parsed, arg: Arg) -> Result[T, CliError]:
        let mut source = CliSource::[T] { parsed: parsed, arg: arg, at: 0 }
        T::build(source)

impl[S] Describer[S] for CliDescriber[S]:
    type Error = never
    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        self.start(v.info)        # a data type: its about line; an enum: a choice, or a command's parser
        .Ok(())
    fn member[F < Cli](mut self, h: Field[S, F]) -> Result[void, never]:
        F::declare(self.current(), arg_for(h))     # name, doc, facts, and required = !h.has_default()
        .Ok(())

impl[S] Source[S] for CliSource[S]:
    type Error = CliError
    fn variant(mut self, choices: List[Variant[S]]) -> Result[Variant[S], CliError]:
        self.choose(choices)      # by the value's name, or by parsed.command()
    fn next(mut self, members: Members[S]) -> Result[Key[S], CliError]:
        self.next_in_order(members)
    fn member[F < Cli](mut self, h: Field[S, F], previous: F?) -> Result[F, CliError]:
        arg := arg_for(h)
        if h.has_default() && !self.parsed.has(arg.name):
            return .Ok(h.default().expect("a declared default"))
        F::read(self.parsed, arg)
    fn missing[F](mut self, h: Field[S, F]) -> Result[F, CliError]:
        panic("std.cli: the source names every member")

impl[T < Cli] Cli for List[T]:
    fn declare(parser: mut ArgParser, arg: Arg) -> void:
        T::declare(parser, Arg { ...arg, repeated: true, required: false })
    fn read(parsed: Parsed, arg: Arg) -> Result[List[T], CliError]:
        let found: mut List[T] = []
        for index in 0..parsed.values(arg.name).len():
            found.push(T::read(parsed.nth(arg.name, index), arg)?)
        .Ok(found)
```

`arg_for`, `has` and `nth` are private to `std.cli`. A data type is a
command whose members declare into one parser. An enum with no payloads
is a value set. An enum with payloads is a set of commands, each with its
own parser, and a unit variant there is a command with no arguments. The
compiler supplies only the traversals it already supplies for serde.

### The Untyped Layer

The builder is renamed `ArgParser`, Dart's and Python's name for it,
because the derive takes the name `Cli`. It becomes a `mut` builder, and
one declaration record, `Arg`, replaces `flag`, `option` and
`positional`, as clap's `Arg` does:

```text
pub data Arg:
    pub name: string
    pub kind: ArgKind = ArgKind.Option        # Flag, Option, Positional
    pub short: char? = .None
    pub help: string = ""
    pub env: string? = .None                  # the variable read when the argument is absent
    pub choices: List[string] = []            # the only values allowed, when not empty
    pub repeated: bool = false
    pub required: bool = false

impl ArgParser:
    pub fn new(program: string, about: string = "") -> mut ArgParser
    pub fn arg(mut self, arg: Arg) -> mut ArgParser
    pub fn command(mut self, name: string, parser: ArgParser) -> mut ArgParser
    pub fn parse(self, arguments: List[string]) -> Result[Parsed, CliError]
    pub fn parse_args(self) -> Result[Parsed, CliError] $ Args + Env
    pub fn usage(self) -> string

impl Parsed:                                  # fields become private
    pub fn flag(self, name: string) -> bool
    pub fn value(self, name: string) -> string?                # the last value
    pub fn values(self, name: string) -> List[string]          # new: every value, in order
    pub fn value_as[T < Parse[Error = E], E < Display](self, name: string) -> Result[T?, CliError]
    pub fn command(self) -> (string, Parsed)?                  # new: the command and what it read

pub enum CliError:
    Help(usage: string)                       # new: -h or --help
    UnknownOption(text: string)
    UnknownCommand(text: string)              # new
    MissingValue(name: string)
    MissingArgument(name: string)             # was MissingPositional; also a required option
    MissingCommand                            # new
    UnexpectedValue(name: string)
    InvalidValue(name: string, reason: string)   # a refused Parse, or a value outside choices
```

- A parser with commands takes no positionals. The first argument that is
  not an option names the command, and the rest goes to its parser, as
  Go's `flag.NewFlagSet` pattern and git do.
- `MissingArgument` names the argument as the usage writes it: `--out`
  or `<file>`.
- Help text lists choices and commands. For the scenario 4 program,
  `notes --help` prints:

```text
usage: notes [options] <command>

Build a static site from Markdown notes.

commands:
  build  Convert the notes to HTML.
  clean  Remove the output directory.

options:
  -v, --verbose  Print each file as it is converted.
  -h, --help     Print this help.
```

A command line known only at run time uses the builder directly. Here a
task runner reads its tasks from a file:

```text
fn task_parser(task: Task) -> mut ArgParser:
    let mut parser = ArgParser::new("run ${task.name}", about=task.description)
    for param in task.params:
        parser.arg(Arg { name: param.name, help: param.help, required: param.default.is_none() })
    parser
```

**Cost.** `parser()` runs one describe, O(fields), and a fact lookup
scans that member's few facts. `parse` reads each argument once and finds
its option in a map by long and short name, O(arguments + declarations).
Today every argument scans every option, and every flag scans the flags
already seen. `read` is one `build` with one map lookup per member, and a
`List[T]` element reads through a one-value view in O(1). `usage` writes
one `StringBuilder`, where today it joins with `+`, quadratic in the
entries. `parse_args` makes one host call per `@from_env` name.

## std.time

**Verdict: meets the bar.** Time values need no builder. The cuts are
duplicates:

- `Duration::milliseconds(n)` and `Duration::seconds(n)` are removed. The
  suffix functions `ms(n)` and `s(n)` are the same calls, and `5s` is
  the literal.
- `ParseDurationError` merges into `TimeParseError`: both are a variant
  and a byte position, and one parse error type serves all time text.

Principle 8 adds three changes. Units are already in every name
(`as_milliseconds`, `from_unix_milliseconds`), and `5s` is a typed
literal.

```text
impl Parse for Duration          # Go's duration text, "1h30m"; replaces Duration::parse
impl Parse for Timestamp         # RFC 3339, the text Display writes; replaces parse_rfc3339
```

- With `Parse`, a duration or a time is a typed command-line field, an
  `env_as::[Duration]` read, or a regex group. Today each is an inherent
  function that none of those can call. Rust's `FromStr` for chrono's
  `DateTime` reads RFC 3339 the same way.
- `Deserialize for Duration` also reads duration text where the format
  holds text, so a JSON config writes `"timeout": "30s"`, not a bare
  `30` whose unit the reader must guess. Writing stays whole
  milliseconds, which the host boundary reads.

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
        request := Request::get("/repos/$repo/issues").query("state", "open").query("page", page)
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
data Login:
    username: string
    password: string
    csrfmiddlewaretoken: string

@derive(Serialize)
data Profile:
    display_name: string

fn rename!(user: string, password: string, name: string) -> Result[void, dyn Error] $ Http:
    let mut site = Client::new().base_url("https://shop.example").timeout(15s)
    _ := site.send!(Request::get("/login"))?
    csrf := site.cookie("/login", "csrftoken").context("the login page set no CSRF cookie")?
    login := Request::post("/login").form(Login { username: user, password: password, csrfmiddlewaretoken: csrf })
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

**4. A site builder with subcommands**, a global flag, a repeated
option, a glob, and output paths derived from input paths. Its tests
show what the types rule out.

```text
use std.cli.{Cli, CliError, positional, short}
use std.fs.{FsError, FsRead, FsWrite, create_dir_all, glob, read_text, remove, write_text}
use std.host.{Args, Env}
use std.path.Path
use std.error.Error

## Build a static site from Markdown notes.
@derive(Cli)
data Notes:
    ## Print each file as it is converted.
    @short('v')
    verbose: bool
    command: NotesCommand

@derive(Cli)
enum NotesCommand:
    ## Convert the notes to HTML.
    Build(BuildOptions)
    ## Remove the output directory.
    Clean(CleanOptions)

@derive(Cli)
data BuildOptions:
    ## Skip paths that contain this text; repeatable.
    @short('x')
    exclude: List[string]
    ## The output directory.
    @short('o')
    out: Path = Path("site")
    ## The files to convert, such as docs/**/*.md.
    @positional
    pattern: string

@derive(Cli)
data CleanOptions:
    ## The output directory.
    @short('o')
    out: Path = Path("site")

fn build!(options: BuildOptions, verbose: bool) -> Result[usize, FsError] $ Console + FsRead + FsWrite:
    create_dir_all!(options.out)?
    let count: usize = 0
    for source in glob!(options.pattern)?:
        if options.exclude.iter().any(fn(text: string) -> bool: "$source".contains(text)):
            continue
        name := source.with_extension("html").file_name().unwrap_or("index.html")
        write_text!(options.out.join(name), "<pre>${read_text!(source)?}</pre>")?
        if verbose:
            println("converted $source")
        count = count + 1
    .Ok(count)

pub fn main!() -> Result[void, dyn Error] $ Console + Args + Env + FsRead + FsWrite:
    notes := Notes::parse_args()?
    match notes.command:
        .Build(options) =>
            count := build!(options, notes.verbose)?
            println("converted $count files into ${options.out}")
        .Clean(options) => remove!(options.out)?
    .Ok(())

tests:
    use std.testing.assert_equal

    it("reads the global flag, the subcommand and its options"):
        match Notes::parse(["-v", "build", "-x", "drafts", "-x", "tmp", "docs/**/*.md"]):
            .Ok(notes) =>
                assert_equal(notes.verbose, true, reason="the flag before the command")
                match notes.command:
                    .Build(options) =>
                        assert_equal(options.exclude, ["drafts", "tmp"], reason="every value, in order")
                        assert_equal(options.out, Path("site"), reason="the default")
                    .Clean(_) => panic("expected build")
            .Err(error) => panic("unexpected $error")

    it("rejects what the types rule out"):
        assert_equal(Notes::parse(["build"]).err(), .Some(CliError.MissingArgument("<pattern>")), reason="a required positional")
        assert_equal(Notes::parse(["publish"]).err(), .Some(CliError.UnknownCommand("publish")), reason="a closed set of commands")
        assert_equal(Notes::parse(["clean", "-x", "tmp"]).err(), .Some(CliError.UnknownOption("-x")), reason="build's option is not clean's")
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

**6. A deploy tool** with repeated options, an option that falls back
to an environment variable, a `Duration` option with a default, and a
field of the program's own type with `Parse`; its test supplies the
arguments and the environment through scripted providers.

```text
use std.cli.{Arg, ArgParser, Cli, CliError, Parsed, from_env, positional, short}
use std.http.{Client, Http, Request}
use std.host.{Args, Env}
use std.serde.Serialize
use std.text.Parse
use std.time.{Duration, min}
use std.error.Error

## A label as key=value.
data Label:
    key: string
    value: string

impl Parse for Label:
    type Error = string
    fn parse(text: string) -> Result[Label, string]:
        match text.split_once("="):
            .Some((key, value)) if key != "" => .Ok(Label { key: key, value: value })
            _ => .Err("expected key=value, got $text")

# A type with its own Parse reads as one value, through the untyped layer.
impl Cli for Label:
    fn declare(parser: mut ArgParser, arg: Arg) -> void:
        parser.arg(arg)
    fn read(parsed: Parsed, arg: Arg) -> Result[Label, CliError]:
        match parsed.value_as::[Label](arg.name)?:
            .Some(label) => .Ok(label)
            .None => .Err(CliError.MissingArgument("--${arg.name}"))

## Deploy one service to one or more regions.
@derive(Cli)
data Deploy:
    ## The API token; DEPLOY_TOKEN when the option is absent.
    @from_env("DEPLOY_TOKEN")
    token: string
    ## A region to deploy to; repeatable; every region when absent.
    @short('r')
    region: List[string]
    ## A label sent with the release; repeatable.
    @short('l')
    label: List[Label]
    ## How long to wait for each region.
    timeout: Duration = 10min
    ## Print the requests instead of sending them.
    dry_run: bool
    ## The service to deploy.
    @positional
    service: string

@derive(Serialize)
data Release:
    service: string
    region: string
    labels: Map[string, string]

pub fn main!() -> Result[void, dyn Error] $ Console + Args + Env + Http:
    deploy := Deploy::parse_args()?
    let mut api = Client::new().base_url("https://deploy.example").timeout(deploy.timeout)
    api.header("Authorization", "Bearer ${deploy.token}")
    let labels: mut Map[string, string] = {}
    for label in deploy.label:
        labels[label.key] = label.value
    regions := if deploy.region.is_empty(): ["eu-west", "us-east"] else: deploy.region
    for region in regions:
        release := Release { service: deploy.service, region: region, labels: labels }
        if deploy.dry_run:
            println("would POST /releases for $region")
            continue
        _ := api.send!(Request::post("/releases").json(release))?
        println("deployed ${deploy.service} to $region")
    .Ok(())

tests:
    use std.host.{ScriptedArgs, ScriptedEnv}
    use std.testing.assert_equal

    it("takes the token from the environment and keeps every repeated value"):
        args := ScriptedArgs::new("deploy", ["-r", "eu-west", "-r", "us-east", "-l", "team=pay", "--dry-run", "billing"])
        env := ScriptedEnv::new({"DEPLOY_TOKEN": "t0k"})
        $.with(Args=args, Env=env):
            match Deploy::parse_args():
                .Ok(deploy) =>
                    assert_equal(deploy.token, "t0k", reason="the environment fallback")
                    assert_equal(deploy.region, ["eu-west", "us-east"], reason="every value, in order")
                    assert_equal(deploy.label[0].value, "pay", reason="a typed label")
                    assert_equal(deploy.timeout, 10min, reason="the default")
                .Err(error) => panic("unexpected $error")

    it("names the option whose value does not parse"):
        bad := Deploy::parse(["--token", "t", "--label", "team", "billing"])
        assert_equal(bad.err(), .Some(CliError.InvalidValue("label", "expected key=value, got team")), reason="the Parse error")
```

**7. A report generator** with two enum options, an optional number,
and two required positionals.

```text
use std.cli.{Cli, CliError, positional, short}
use std.fs.{FsRead, FsWrite, read_text, write_text}
use std.host.{Args, Env}
use std.json.encode
use std.path.Path
use std.serde.Serialize
use std.text.StringBuilder
use std.error.{Error, message}

@derive(Cli)
enum Format:
    ## A JSON array.
    Json
    ## A Markdown table.
    Markdown

@derive(Cli)
enum SortKey:
    ## Newest first.
    Date
    ## Largest first.
    Total

## Turn an orders CSV into a sorted report.
@derive(Cli)
data Report:
    ## The output format.
    @short('f')
    format: Format
    ## The order of the rows.
    @short('s')
    sort: SortKey = SortKey.Date
    ## Keep only orders of at least this many cents.
    min_cents: i64?
    ## The orders CSV, one date,customer,cents line per order.
    @positional
    input: Path
    ## The report to write.
    @positional
    output: Path

@derive(Serialize)
data Order:
    date: string
    customer: string
    cents: i64

fn read_orders(text: string) -> Result[List[Order], dyn Error]:
    let orders: mut List[Order] = []
    for (index, line) in text.lines().iter().enumerate():
        fields := line.split(",")
        if fields.len() != 3:
            return .Err(message("line ${index + 1}: expected date,customer,cents"))
        cents := fields[2].parse::[i64]().context("line ${index + 1}: bad cents")?
        orders.push(Order { date: fields[0], customer: fields[1], cents: cents })
    .Ok(orders)

fn render(orders: List[Order], format: Format) -> string:
    match format:
        .Json => encode(orders)
        .Markdown =>
            let out: mut StringBuilder = StringBuilder::new()
            out.push("| date | customer | cents |\n| --- | --- | ---: |\n")
            for order in orders:
                out.push("| ${order.date} | ${order.customer} | ${order.cents} |\n")
            out.build()

pub fn main!() -> Result[void, dyn Error] $ Args + Env + FsRead + FsWrite:
    report := Report::parse_args()?
    let orders = read_orders(read_text!(report.input)?)?
    match report.min_cents:
        .Some(floor) => orders = orders.iter().filter(fn(order: Order) -> bool: order.cents >= floor).collect()
        .None => pass
    sorted := match report.sort:
        .Date => orders.sorted_by(fn(a: Order, b: Order) -> Ordering: b.date.cmp(a.date))
        .Total => orders.sorted_by(fn(a: Order, b: Order) -> Ordering: b.cents.cmp(a.cents))
    write_text!(report.output, render(sorted, report.format))?
    .Ok(())

tests:
    use std.testing.assert_equal

    it("checks the enum and the positionals before the program runs"):
        html := Report::parse(["-f", "html", "orders.csv", "report.md"])
        assert_equal(html.err(), .Some(CliError.InvalidValue("format", "expected json or markdown, got html")), reason="a closed set")
        one := Report::parse(["-f", "json", "orders.csv"])
        assert_equal(one.err(), .Some(CliError.MissingArgument("<output>")), reason="both positionals are required")
        bare := Report::parse(["orders.csv", "report.md"])
        assert_equal(bare.err(), .Some(CliError.MissingArgument("--format")), reason="no default, so required")
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
| `std.cli` | 13 | 28 | the builder `Cli` and its `new`, `flag`, `option`, `positional`, `parse`, `parse_args`, `usage` | the trait `Cli` and its `declare`, `read`, `parser`, `parse`, `parse_args`; `Short`, `short`, `FromEnv`, `from_env`, `Positional`, `positional`; `ArgParser` and its `new`, `arg`, `command`, `parse`, `parse_args`, `usage`; `Arg`, `ArgKind`; `Parsed.values`, `command` |
| `std.time` | 35 | 30 | `Duration::milliseconds`, `seconds`, `parse`, `ParseDurationError`, `Timestamp::parse_rfc3339` | none (the `Parse` impls are not names) |
| **Total** | **204** | **246** | **25** | **67** |

The growth is in HTTP, processes and the CLI. Today's HTTP and process
surface could not express auth, cookies, queries, forms, a working
directory or an environment. In `std.cli`, 12 names are the typed layer
(the trait, its methods and the three facts), and 16 are the untyped
layer it is built on, which also gains subcommands, choices, required
options and environment fallbacks. Most added names appear in a
scenario above; the rest (`body`, `redirects`, the free net helpers,
`copy!`, `file_stem`, `print`) each close a gap the bar names. Each
removed name is a fixed combination, a duplicate, an inherent parse
that a `Parse` impl replaces, or a builder method that `Arg` replaces.

## Questions For The Owner

1. **Constructor spelling.** `Request::get(url)` keeps one rule for every
   builder (`Command::new`, `ArgParser::new`, `Client::new`), while free
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
6. **`std.cli` as a typed derive.** `@derive(Cli)` reads the command
   line into the program's own type, built on the untyped `ArgParser`,
   `Arg` and `Parsed`. Taking the name `Cli` for the derive renames the
   builder to `ArgParser`. The other choice keeps the builder as `Cli`
   and names the derive `FromArgs`, as argh does. Recommendation: the
   derive as proposed, with the short name on the common path.
7. **How `--help` ends a program.** `parse_args()?` returns
   `CliError.Help(usage)`, so the host prints the help on standard error
   and exits 1. clap, Python's `argparse` and Go's `flag` print it and
   exit 0. A program that cares matches `.Help`, prints it, and returns
   `.Ok(())`. A fix in the language tier would let an error choose its
   exit status, for example an `Error` method with default 1 that
   `Termination` reads. Recommendation: accept exit 1 now, and take the
   exit status question with the next language-tier batch.

## Data Modules

Job 2 covers the modules that do no I/O: `std.json`, `std.serde`,
`std.text`, `std.format`, `std.regex`, `std.encoding`, `std.digest`,
`std.hash`, `std.random`, `std.num` and `std.cmp`. They follow the shape
above where it applies, with three readings for pure code:

- **No terminal.** A pure call returns its value; a builder exists only
  where input arrives in pieces (`StringBuilder`, `Sha256`). Text becomes
  a value through a typed call: `text.parse::[T]()`, `decode::[T](text)`,
  `regex.decode::[T](text)`.
- **Typed by default, untyped underneath.** The typed call is built on
  the untyped layer and both stay public: `decode::[T]` reads a `Json`,
  and `Regex.decode::[T]` reads `Captures`. The untyped layer is the
  escape hatch for data whose shape is known only at run time.
- **Errors.** One enum per module. A text error carries `position: usize`,
  a byte offset, in each variant that has one; a decode error carries the
  path or group it failed at, as `JsonError.WrongType(path, expected)`.
  `NotGranted` and `Other` are host-module variants and do not apply.

## std.json

**Verdict: meets the bar, with one cut and one addition.** The typed
path, `decode::[T](text)` and `encode(value)`, is the default and is one
call each; `Json` is the escape hatch, and `to_json` and `from_json`
join the two, as serde_json's `to_value` and `from_value` do. A missing
key takes the member's default. The real gap is naming: real APIs write
`createdAt`, which a derived type cannot match today (question 1).

```text
impl JsonObject:
    pub fn remove(mut self, key: string) -> Json?     # new; the other members keep their order
```

- Removed: `Json.as_number()`. `as_i64`, `as_u64` and `as_f64` read a
  number in one step, and `match value: .Number(n) => ...` still gives
  the `Number`.
- Added: `JsonObject.remove`, for editing a parsed config, as serde_json's
  `Map::remove`.

**Cost.** `parse` is one pass, and a string without escapes shares the
input's bytes. `decode` builds the whole `Json` tree and then walks it,
which allocates every value twice. The API does not force that: a reader
from text straight to `T`, as serde_json's `from_str`, can replace it
behind `decode`. `encode` writes text in one walk, but an irregular walk
serializes twice. Each number in `Display` makes its own string.

## std.serde

**Verdict: the protocol meets the bar; the derive misses the real 80%.**
The data model is serde's, cut to 14 writer calls, and the derive is the
typed path that every format reads. Two gaps stop real programs:

- Member names cannot differ from field names, and `.None` always writes
  `null`, so a camelCase API needs a `Json` walk instead of a derived
  type (question 1). The planned facts in
  [Serialization Formats](OPEN_ISSUES.md#serialization-formats) cover
  `rename` and `skip`; serde's `rename_all` and `skip_serializing_if`
  are the two they lack.
- `Set[T]` and tuples have no implementation. serde writes both as
  lists; the tuple template makes tuples plain hd.

```text
@serde(rename_all="camelCase")           # hypothetical syntax: the planned facts
@derive(Serialize, Deserialize)
data Order:
    order_id: string                     # "orderId"
    @serde(rename="total")               # hypothetical syntax
    total_cents: i64
    @serde(skip_none)                    # hypothetical syntax
    coupon: string?
```

No public name changes.

**Cost.** A fact is read through the `Member` handle, which the derive
builds once per type, so a rename costs nothing per value. Today a
derived `deserialize` builds a list of variant facts on every value it
reads.

## std.text

**Verdict: meets the bar, with one cut and two fixes.** Strings stay
Go's: immutable UTF-8, sliced without copying, built with
`StringBuilder`. `Parse` with `text.parse::[T]()` is the typed path, as
Rust's `FromStr` is.

```text
impl StringBuilder:
    pub fn push_char(mut self, c: char) -> void        # new

pub enum Utf8Error:
    InvalidSequence(position: usize)
    Truncated(position: usize)                         # was Truncated, with no position
```

- Removed: the free `join(parts, separator)`. `parts.join(separator)` is
  the same call, and the free one was kept only for old imports.
- `push_char` exists for cost: today a character goes in as `"$c"`,
  which builds a list and a string for one scalar. Go's `WriteRune` and
  Rust's `String::push` are the same atom.
- `Truncated(position)` gives every `Utf8Error` a position, as the
  error rule above asks.

**Cost.** `trim`, `split_once`, `strip_prefix` and the pieces of `split`
and `lines` share the string's bytes. `split` and `lines` allocate one
list; `lines` today builds two. `StringBuilder` keeps its pieces and
copies once in `build`. `to_utf8` copies byte by byte, and `pad_start`
copies the result twice.

## std.format

**Verdict: meets the bar.** The `Debug` builders are Rust's `Formatter`
builders, and `debug` and `dbg` are one call each. hd has no format-spec
strings, and none are proposed: a number is formatted by typed methods
that compose with padding, as Go's `strconv` does beside `fmt`, so
`id.to_radix(16).pad_start(8, '0')` and `price.to_fixed(2).pad_start(9)`
replace `{:08x}` and `{:>9.2}`. No public name changes.

**Cost.** `Display.to_string` returns a new string per value, so a
`Display` that interpolates its parts copies each part's text once per
level of nesting (question 2). An integer's text is built by prepending
one digit at a time, which the owner's `format_int` decision fixes. The
pretty `Debug` layout rebuilds its indentation with `+` on every field.

## std.regex

**Verdict: the engine meets the bar; the results are untyped.** The
shape is RE2's, Go's and Rust's, every search is linear, and named
groups exist. But the 80% use of a named group is to read a record out
of a line, and today each field is `caps.name("status")`, then an
option, then a parse, then an error by hand.

The proposal reads the groups into a derived type, as Rust's `recap`
crate does over serde and Python's `parse` library does over a format.
`Captures` stays as the escape hatch, and `decode` is built on it.

```text
impl Regex:
    pub fn decode[T < Deserialize](self, text: string) -> Result[T, RegexError]   # new
impl Captures:
    pub fn decode[T < Deserialize](self) -> Result[T, RegexError]                 # new

pub enum RegexError:                                     # replaces RegexError and RegexErrorKind
    MissingParen(position: usize)
    ...                                                  # the other ten kinds, each with a position
    NoMatch
    MissingGroup(name: string)
    WrongType(group: string, expected: string)
```

- `decode` reads each member from the group of the same name. A
  `string` member is a slice of the text; an integer, float, `bool` or
  `char` member parses the group as `text.parse::[T]()` does; a `T?`
  member is `.None` when the group took no part; an enum with no
  payloads reads a variant by its name. Anything else is `WrongType`.
- `Regex.decode` is `captures`, then `Captures.decode`, with `NoMatch`
  when nothing matches; `captures_all` with `Captures.decode` reads every
  record in a text. This mirrors `decode` and `from_json` in `std.json`.
- `RegexErrorKind` is removed: one enum, with a position in each pattern
  variant, is the shape `JsonError` and `DecodeError` already have.
- Still missing: `(?i)` and `\b`, deferred by an earlier stdlib call.
  They hurt log searches most, and they add syntax, not names.

**Cost.** `decode` runs one search, then finds each member's group by
name among the g groups, so O(g) per member. String members copy
nothing, and numbers parse from the slice. Each search allocates the
VM's tables, which one allocation per `Regex` could serve. `replace`
joins three pieces, and expanding `$name` references concatenates once
per reference, which is quadratic in their number.

## std.encoding

**Verdict: hex meets the bar; base64 does not.** Base64 has only the
standard padded alphabet, so a JWT, a WebAuthn ID or a URL token cannot
be read. Adding `base64url_encode` and its kin would be four fixed
combinations of alphabet and padding. The proposal is Go's
`base64.Encoding` values, which Rust's `base64` engines also follow: the
set is closed, so it is an enum.

```text
pub enum Base64:
    Standard                  # RFC 4648 section 4, padded: MIME, Basic auth
    StandardNoPad
    Url                       # section 5, padded
    UrlNoPad                  # JWT, tokens

impl Base64:
    pub fn encode(self, bytes: List[u8]) -> string
    pub fn decode(self, text: string) -> Result[List[u8], DecodeError]

pub fn hex_encode(bytes: List[u8]) -> string            # unchanged
pub fn hex_decode(text: string) -> Result[List[u8], DecodeError]
```

- Removed: `base64_encode` and `base64_decode`, now
  `Base64.Standard.encode` and `Base64.Standard.decode`.
- Decoding stays strict for every variant. A `NoPad` variant rejects
  `=`, and a padded one requires it, as Go's `Strict` mode does.

**Cost.** The output length is known from the input, so each call can
fill one buffer of exactly that size. Today encoding builds a list and
then copies it into a string, and decoding first copies the text into a
list of `i32`.

## std.digest

**Verdict: fails the bar.** `sha256_hex` fixes one combination of
`sha256` and hex. The one-shot `sha256` needs the whole message in
memory. There is no HMAC, which webhooks and signed tokens need. The
result is a bare `List[u8]`, whose `==` returns at the first differing
byte and so leaks timing to a forged signature.

The proposal is Go's `hash.Hash` (`Write`, then `Sum`) and Rust's
`Digest` (`new`, `update`, `finalize`). The key goes in the constructor,
as BLAKE3's `Hasher::new_keyed` does. A digest compares in constant
time, as Go's `hmac.Equal` does.

```text
pub data Digest:
    pub bytes: List[u8]

impl Digest:
    pub fn to_hex(self) -> string
impl Eq for Digest                       # constant time over the bytes

pub data Sha256: ...                     # private: state, the partial block, length

impl Sha256:
    pub fn new() -> mut Sha256
    pub fn hmac(key: List[u8]) -> mut Sha256        # finish gives HMAC-SHA256 under key
    pub fn update(mut self, bytes: List[u8]) -> mut Sha256
    pub fn finish(self) -> Digest

pub fn sha256(bytes: List[u8]) -> Digest
```

- Removed: `sha256_hex`, now `sha256(bytes).to_hex()`.
- A received signature is `Digest { bytes: hex_decode(text)? }`, so the
  check is `expected == sent`, in constant time, with no new name.

**Cost.** `update` fills a 64-byte block and compresses each full block
in place, so the message is never copied. Today `sha256` copies the
whole message to pad it, and each of its about 450 word additions per
block runs `wrapping_add` as a loop over carry bits.

## std.hash

**Verdict: meets the bar.** `Hash` derives, `DefaultHasher` is a fixed
FNV-1a, and `hash_of` is the one-call path. No public name changes.

**Cost.** The `Hasher` trait has only `write(bytes: List[u8])`, so
hashing an integer allocates a list for its bytes and hashing a string
copies its bytes first (question 3). Each write is also an indirect call
through `dyn Hasher`.

## std.random

**Verdict: meets the bar, with one cut.** `Rng` is Go's `math/rand/v2`
generator value, and `rng()` seeds it with one host draw. `token_hex`
fixes one combination of `fill` and hex. `fill` has no free helper,
which the shared convention asks of every host-trait method.

```text
pub fn fill(count: usize) -> List[u8] $ Random       # new; replaces token_hex
```

- Removed: `token_hex(count)`, now `hex_encode(fill(count))`, or
  `Base64.UrlNoPad.encode(fill(count))` for a shorter token.
- `rng()` stands in for a free `next_u64`, since a program that wants
  many draws should hold one generator.
- `int` takes only a `Range[i64]` and returns an `i64`, so picking an
  index or a `u8` needs a cast (question 4).

**Cost.** Every `Rng` method is arithmetic on four words, with no
allocation. `sample` copies the whole list even to pick 3 of a million.
`SeededRandom.fill` spends one 32-bit draw per byte.

## std.num

**Verdict: the arithmetic meets the bar; parsing is duplicated.** The
method names are Rust's. Since `Parse` exists, `parse_i32`, `parse_i64`,
`parse_u32`, `parse_u64`, `parse_usize` and `parse_f64` are the same
calls as `text.parse::[i32]()` and its kin, and Rust keeps only
`FromStr`. Four atoms are missing.

```text
impl[N < Integer] N:
    pub fn wrapping_mul(self, other: N) -> N                   # new
    pub fn to_radix(self, radix: u32) -> string                # new: "ff" for 255 and 16
    pub fn parse_radix(text: string, radix: u32) -> Result[N, ParseNumberError]   # new

impl f64:                                                       # and f32
    pub fn total_cmp(self, other: f64) -> Ordering             # new
    pub fn sqrt(self) -> f64                                   # the owner's intrinsic decision
```

- Removed: the six `parse_` functions. `ParseNumberError` stays as the
  `Error` of every number's `Parse`.
- `wrapping_mul` serves hashes and generators. `to_radix` and
  `parse_radix` serve hex IDs, colors and file modes, as Go's
  `FormatInt` and `ParseInt` do; a radix outside 2 to 36 panics, as
  `to_digit` does. `total_cmp` sorts floats, which are not `Ord`.
- `is_nan` and `is_finite` gain `f32` impls, with no new name.

**Cost.** `wrapping_add`, `count_ones`, `leading_zeros` and the
rotations loop over bits, where Wasm has one instruction for each.
`checked_mul` divides twice.

## std.cmp

**Verdict: meets the bar, with one addition.** `min`, `max`, `clamp`
and `Reverse` are Rust's. A comparator over two keys today nests a
`match`; Rust's `Ordering::then` chains it.

```text
impl Ordering:
    pub fn then(self, other: Ordering) -> Ordering    # self unless .Equal, then other
```

**Cost.** String comparison reads one byte at a time with a bounds
check on each, where one memory compare would do.

## Data Scenarios

Each program uses only the proposed surface and parses with
`scripts/hd-conformance.sh parse`. None checks today.

**1. A signed webhook.** Check an HMAC signature from a header in
constant time, then decode a camelCase payload into a type.

```text
use std.digest.{Digest, Sha256}
use std.encoding.hex_decode
use std.json.decode
use std.serde.Deserialize
use std.time.Timestamp
use std.error.{Error, message}

@serde(rename_all="camelCase")              # hypothetical syntax: question 1
@derive(Deserialize)
data OrderPaid:
    order_id: string
    total_cents: i64
    currency: string
    coupon: string?
    created_at: Timestamp

fn verify(secret: string, body: string, header: string) -> Result[void, dyn Error]:
    hex := header.strip_prefix("sha256=").context("the signature has no sha256= prefix")?
    sent := Digest { bytes: hex_decode(hex)? }
    expected := Sha256::hmac(secret.to_utf8()).update(body.to_utf8()).finish()
    if expected != sent:
        return .Err(message("the signature does not match"))
    .Ok(())

pub fn on_order_paid(secret: string, body: string, header: string) -> Result[OrderPaid, dyn Error]:
    verify(secret, body, header)?
    .Ok(decode::[OrderPaid](body)?)

tests:
    use std.testing.assert_equal

    it("accepts a signed body and rejects a changed one"):
        body := "{\"orderId\":\"o-7\",\"totalCents\":1299,\"currency\":\"EUR\",\"createdAt\":\"2026-10-17T09:00:00Z\"}"
        header := "sha256=${Sha256::hmac("k".to_utf8()).update(body.to_utf8()).finish().to_hex()}"
        match on_order_paid("k", body, header):
            .Ok(order) => assert_equal(order.total_cents, 1299, reason="decoded")
            .Err(error) => panic("unexpected $error")
        changed := body.replace("1299", "1")
        assert_equal(on_order_paid("k", changed, header).is_err(), true, reason="tampered")
```

**2. An access-log report**: typed records from a regex, malformed lines
counted, the busiest paths sorted by two keys, and a p95 latency.

```text
use std.regex.Regex
use std.serde.Deserialize
use std.collections.counts
use std.fs.{FsRead, read_text}
use std.path.Path
use std.text.r
use std.error.{Error, message}

@derive(Deserialize)
data Hit:
    ip: string
    method: string
    path: string
    status: u16
    bytes: u64
    seconds: f64
    referrer: string?

fn pattern() -> string:
    r"^(?P<ip>\S+) \S+ \S+ \[[^\]]+\] \"(?P<method>[A-Z]+) (?P<path>\S+) [^\"]*\" (?P<status>\d{3}) (?P<bytes>\d+) (?P<seconds>\d+\.\d+)(?: \"(?P<referrer>[^\"]*)\")?$"

fn read_hits(log: string) -> Result[(List[Hit], usize), dyn Error]:
    line := Regex::new(pattern())?
    let hits: mut List[Hit] = []
    let skipped: usize = 0
    for (index, text) in log.lines().iter().enumerate():
        match line.decode::[Hit](text):
            .Ok(hit) => hits.push(hit)
            .Err(.NoMatch) => skipped = skipped + 1
            .Err(error) => return .Err(message("line ${index + 1}: $error"))
    .Ok((hits, skipped))

fn by_count(a: (string, usize), b: (string, usize)) -> Ordering:
    b._1.cmp(a._1).then(a._0.cmp(b._0))

pub fn main!() -> Result[void, dyn Error] $ Console + FsRead:
    let (hits, skipped) = read_hits(read_text!(Path("access.log"))?)?
    failed := hits.iter().filter(fn(hit: Hit) -> bool: hit.status >= 500).count()
    paths := counts(hits.map(fn(hit: Hit) -> string: hit.path))
    for (path, count) in paths.entries().sorted_by(by_count).take(5):
        println("${"$count".pad_start(7)}  $path")
    times := hits.map(fn(hit: Hit) -> f64: hit.seconds).sorted_by(fn(a: f64, b: f64) -> Ordering: a.total_cmp(b))
    if !times.is_empty():
        println("p95 ${times[times.len() * 95 / 100].to_fixed(3)}s, $failed failed, $skipped skipped")
    .Ok(())

tests:
    use std.testing.assert_equal

    it("reads a line into a Hit"):
        log := "1.2.3.4 - - [17/Oct/2026:09:00:00 +0000] \"GET /api/items HTTP/1.1\" 503 120 0.250\nnoise\n"
        match read_hits(log):
            .Ok((hits, skipped)) =>
                assert_equal(hits[0].status, 503, reason="a typed status")
                assert_equal(hits[0].referrer, .None, reason="the group took no part")
                assert_equal(skipped, 1, reason="one malformed line")
            .Err(error) => panic("unexpected $error")
```

**3. Experiments and secrets**: stable bucketing by hash, a seeded audit
sample that repeats everywhere, and a session token from the host.

```text
use std.hash.hash_of
use std.random.{Random, Rng, fill, rng}
use std.encoding.Base64

pub fn arm(experiment: string, user: string, arms: u64) -> u64:
    hash_of((experiment, user)) % arms

pub fn audit_sample(seed: u64, orders: List[string]) -> List[string]:
    let mut gen = Rng::new(seed)
    gen.sample(orders, 3)

pub fn session_token() -> string $ Random:
    Base64.UrlNoPad.encode(fill(32))

pub fn roll_out(share: f64) -> bool $ Random:
    let mut gen = rng()
    gen.float() < share

tests:
    use std.random.SeededRandom
    use std.testing.assert_equal

    it("buckets stably and samples reproducibly"):
        assert_equal(arm("checkout-v2", "u-81", 2), arm("checkout-v2", "u-81", 2), reason="stable")
        orders := ["o1", "o2", "o3", "o4", "o5", "o6"]
        assert_equal(audit_sample(7, orders), audit_sample(7, orders), reason="one seed, one sample")
        let mut source = SeededRandom::new(1)
        $.with(Random=source):
            assert_equal(session_token().len(), 43, reason="32 bytes, unpadded")
```

**4. An invoice export**: typed parsing of input rows with error context,
hex IDs, money with two decimals, and CSV quoting with a builder.

```text
use std.text.StringBuilder
use std.error.{Error, message}

data Line:
    sku: u32
    name: string
    quantity: u32
    cents: i64

fn read_line(row: string) -> Result[Line, dyn Error]:
    fields := row.split(",")
    if fields.len() != 4:
        return .Err(message("expected sku,name,quantity,cents: $row"))
    sku := u32::parse_radix(fields[0], 16).context("bad sku ${fields[0]}")?
    quantity := fields[2].parse::[u32]().context("bad quantity in $row")?
    cents := fields[3].parse::[i64]().context("bad price in $row")?
    .Ok(Line { sku: sku, name: fields[1], quantity: quantity, cents: cents })

fn quoted(text: string) -> string:
    if !text.contains(",") && !text.contains("\"") && !text.contains("\n"):
        return text
    let out: mut StringBuilder = StringBuilder::new()
    out.push_char('"')
    for c in text.chars():
        if c == '"':
            out.push_char('"')
        out.push_char(c)
    out.push_char('"')
    out.build()

pub fn export(rows: List[string]) -> Result[string, dyn Error]:
    let out: mut StringBuilder = StringBuilder::new()
    out.push("sku,name,quantity,total\n")
    for row in rows:
        line := read_line(row)?
        total := f64(line.cents * i64(line.quantity)) / 100.0
        out.push("${line.sku.to_radix(16).pad_start(8, '0')},${quoted(line.name)},${line.quantity},${total.to_fixed(2)}\n")
    .Ok(out.build())
```

## Data Public Names

Counted as above, with one more rule: a method declared for every
numeric width counts once. "Before" is `lib/std` at `293b78417`.

| Module | Before | After | Removed | Added |
| --- | --- | --- | --- | --- |
| `std.json` | 34 | 34 | `Json.as_number` | `JsonObject.remove` |
| `std.serde` | 37 | 37 | none | none (facts and impls are not names) |
| `std.text` | 51 | 51 | free `join` | `StringBuilder.push_char` |
| `std.format` | 25 | 25 | none | none |
| `std.regex` | 18 | 19 | `RegexErrorKind` | `Regex.decode`, `Captures.decode` |
| `std.encoding` | 5 | 6 | `base64_encode`, `base64_decode` | `Base64` and its `encode`, `decode` |
| `std.digest` | 2 | 8 | `sha256_hex` | `Digest`, `to_hex`; `Sha256` and its `new`, `hmac`, `update`, `finish` |
| `std.hash` | 8 | 8 | none | none |
| `std.random` | 16 | 16 | `token_hex` | free `fill` |
| `std.num` | 32 | 31 | `parse_i32`, `parse_i64`, `parse_u32`, `parse_u64`, `parse_usize`, `parse_f64` | `wrapping_mul`, `to_radix`, `parse_radix`, `total_cmp`, `sqrt` |
| `std.cmp` | 11 | 12 | none | `Ordering.then` |
| **Total** | **239** | **247** | **13** | **21** |

The growth is in `std.digest`, where today's two names could not stream,
key a hash, or compare a signature safely. The typed regex path adds
two names over the untyped one it is built on. Each removal is a
duplicate (`join`, the `parse_` functions, `as_number`), a fixed
combination (`sha256_hex`, `token_hex`), or a type folded into another
(`RegexErrorKind`, the `base64_` pair).

## Data Questions For The Owner

**Answered by the owner, 2026-10-17:** all five approved: (1) the serde naming facts `rename`, `skip`, `rename_all` and `skip_none`; (2) `Display` writes into a builder (`write(self, out: mut StringBuilder)` with a default body over `to_string()`, called by interpolation); (3) `Hasher.write_u64` and `write_str` with default bodies; (4) `u64` conversions on the sealed `Integer` so `Rng.int` is generic; (5) regex decoding through `Deserialize`.

1. **Serde naming facts.** Apply the planned `@serde(rename=...)` and
   `@serde(skip)`, and add serde's type-level `rename_all` and a
   `skip_none`, so a derived type reads a camelCase API. They are
   language-tier annotation facts. Recommendation: approve all four; they
   are the largest gap left in the typed JSON path.
2. **`Display` writes into a builder.** `to_string` makes a new string
   per value, so nested `Display` copies text once per level. Rust's
   `Display` writes into a `Formatter`; Go's `String()` returns a string
   and accepts the copy. Recommendation: add `write(self, out: mut
   StringBuilder) -> void` with a default body that pushes `to_string()`,
   and have interpolation call `write`. No existing impl changes.
3. **`Hasher` width methods.** Add `write_u64(value)` and
   `write_str(text)` to the language-tier `Hasher`, with default bodies
   over `write`, as Rust's `Hasher` has. Recommendation: approve; it
   removes a list allocation from every integer key and a copy from
   every string key.
4. **A generic `Rng.int`.** `int[N < Integer](range: Range[N]) -> N`
   needs the sealed `Integer` trait to convert to and from `u64`.
   Recommendation: add those two conversions to `Integer`, so
   `gen.int(0..items.len())` returns a `usize`.
5. **Regex decode through `Deserialize`.** Reading groups through the
   serde derive means a type that decodes JSON also decodes a regex
   match, with the same facts. A separate derive would keep the two
   apart. Recommendation: reuse `Deserialize`, as `recap` does.

## Structures And Control

Job 3 covers the modules that shape values and control flow, and the
prelude. Most do no I/O, so the data modules' readings apply, with three
more:

- **Eager lists, lazy iterators.** A `List` method returns a new list, as
  Kotlin's `List` methods do; an `Iterator` adapter is lazy, as Kotlin's
  `Sequence` is. Both use the same names, so a chain moves between them
  with `.iter()` and `.collect()`. Lists are the 80%; the iterator is the
  20%, for long chains and large or endless sources.
- **Policies are data.** A combinator's options are one data value with
  defaults, as Effect's `Schedule` is: `Backoff { attempts: 5 }`. Setters
  are kept for values that grow lists, such as `Request` and `Command`.
- **Protocols for derive authors.** `std.structure`, `std.annotation` and
  `std.inspect` serve the people who write derives. They are judged on
  cost and on whether they let an invalid state be written.

## std.collections

**Verdict: meets the bar, with one swap and two cost fixes.** The names
are Rust's `Vec`, `VecDeque`, `HashSet` and `BinaryHeap`, with Python's
`Counter.most_common`. Two calls force a cost that no implementation can
remove, and one name is a second spelling.

```text
impl[T] List[T]:
    pub fn chunks(self, size: usize) -> List[ListView[T]]                     # was List[List[T]]
    pub fn windows(self, size: usize) -> List[ListView[T]]                    # was List[List[T]]
    pub fn flat_map[U, I < Iterable[U]](self, transform: fn(T) -> I) -> List[U]   # was fn(T) -> List[U]

impl[K < Eq & Hash, V] Map[K, V]:
    pub fn get_or_insert(mut self, key: K, value: V) -> V                     # new; replaces get_or

impl[T] FromIterator[T] for Deque[T]                                          # new impls, no new names
impl[T < Ord] FromIterator[T] for Heap[T]                                     # heapify in O(n)
```

- `chunks` and `windows` return views. Today `windows(k)` copies every
  element k times; a view copies nothing, as Rust's `chunks` and
  `windows` hand out slices. `to_list()` makes a list where one is
  needed.
- `flat_map` takes any `Iterable` result, as Rust's `IntoIterator` bound
  does, so the function need not build a list per item. With `T?`
  iterable (see std.iter), `flat_map` also keeps the `.Some` results,
  which is `filter_map` with no new name.
- Removed: `Map.get_or(key, fallback)`. It is `get(key).unwrap_or(fallback)`.
- Added: `get_or_insert`, Python's `setdefault` and Rust's
  `entry(key).or_insert(value)`. It returns the stored value, so
  `index.get_or_insert(word, Set::new()).insert(doc)` is one probe, where
  today it is a `get`, a `match` and a store.
- Not proposed: set algebra, which is a `filter` over `contains`; an
  in-place `sort`, since `sorted` costs one copy beside an O(n log n)
  sort; a sorted map.

**Cost.** A list method allocates at most its one result list, of a size
known or bounded by the input. `keys`, `values` and `entries` copy a
snapshot by contract, one list of exact size. Today's code falls short:
`sorted_by` allocates three lists per merge level, `group_by` builds its
map twice, `counts` probes twice per item, `most_common(n)` sorts every
entry to keep n, `Set` stores a `bool` per element, `Heap` and `Deque`
wrap each slot in a `T?`, and `insert` and `remove_at` shift one element
per step. All are filed for #426.

## std.iter

**Verdict: meets the bar, with one fix.** `Iterator[T]` is Rust's adapter
set in pull form. `from_fn` and `next` are the atoms, and `collect`
builds any `FromIterator`. The fix is the `flat_map` bound above, plus
one impl:

```text
impl[T] Iterable[T] for T?                       # new impl: zero or one item
impl[T] Iterator[T]:
    pub fn flat_map[U, I < Iterable[U]](mut self, transform: fn(T) -> I) -> mut Iterator[U]
```

- `text.lines().iter().flat_map(parse)`, with `parse(line) -> Hit?`,
  keeps the lines that parse, as Rust's `flat_map` over an `Option` does;
  `filter_map` and Kotlin's `mapNotNull` are second names hd does not
  need. As in Rust, `for x in maybe:` then also works.

**Cost.** Each adapter allocates one closure, and each item makes one
indirect call per adapter, as Go's `iter.Seq` does. Adapters are small
and do not escape a chain that ends in `collect` or `for`, so inlining
can remove both; the API forces neither. `Set.iter()` adds an adapter
over its map, and a range's `iter()` builds a closure where a counted
loop would do.

## std.option and std.result

**Verdict: meets the bar, with one addition and one fix.** The names are
Rust's, cut to what programs use: `map`, `and_then`, `unwrap_or`,
`expect`, the variant tests, `ok_or`, `ok`, `err`, `map_err`, and
`context` on both. There is no bare `unwrap`, so every panic carries a
message.

```text
impl[T] T?:
    pub fn or(self, other: T?) -> T?                 # new: self when .Some, else other

impl[T, E < Display] Result[T, E]:
    pub fn expect(self, message: string) -> T        # the panic now shows "message: error"
```

- `or` layers fallbacks. A flag, then a variable, then a default is
  `flag.or(env("PORT")).unwrap_or("8080")`. Rust's `Option::or`,
  Kotlin's `?:` and Swift's `??` serve it; today it is a nested `match`.
- `Result.expect` shows only its message, though its doc promises the
  error, which Rust's prints. The bound is `Display`, which every `Error`
  has.
- Not proposed: `unwrap_or_else` and `or_else`. A costly fallback is a
  `match`, which is lazy.

**Cost.** Each method is one `match` and allocates nothing beyond its
result. `unwrap_or(f())` runs `f` even for a `.Some`; that is the
caller's choice, and `match` is the lazy form.

## std.error

**Verdict: meets the bar, with two cuts.** `@error` with `@from` and
`@source` is thiserror. `context` and `message` are anyhow's `context`
and `anyhow!`. `dyn Error` with `chain`, `root_cause` and `find::[T]()`
reads a cause chain by type, as Go's `errors.As` does. `report_of` makes
the boundary-safe `ErrorReport`.

- Removed: the free `chain(error)`, a second spelling of `error.chain()`.
- `ContextError` becomes private, as `MessageError` already is and as
  anyhow keeps both. Its fields are private, so a program can name the
  type but read nothing from it.
- The derive marks a cause with `@source`, while the trait method is
  `cause()`, the report field is `causes`, and the printed line is
  `caused by:` (question 3).

**Cost.** `chain()` builds a list of the causes, and `find` and
`root_cause` build that list before they walk it; a walk over `cause()`
needs none. `report_of` makes one string per cause, which is its purpose.

## std.task

**Verdict: mostly meets the bar.** `timeout!`, `map_all!` and `race!` are
Effect's `timeout`, `forEach` with `concurrency`, and `race`, over
structured suspension: cancelling a call cancels its children. Two names
are redundant.

```text
pub data Backoff:
    pub attempts: usize = 3
    pub initial: Duration = 100ms
    pub factor: u32 = 2                      # was i32, and no field had a default
    pub max: Duration = 10s

pub fn retry![T, E, $R](policy: Backoff, when: fn(E) -> bool = fn(error: E): true, attempt: fn!() -> Result[T, E] $ R) -> Result[T, E] $ R + Clock
pub fn timeout![T, $R](limit: Duration, work: fn!() -> T $ R) -> Result[T, TimedOut] $ R + Clock     # unchanged
pub fn map_all![T, U, $R](items: List[T], limit: usize, work: fn!(T) -> U $ R) -> List[U] $ R           # unchanged
```

- `retry!` and `retry_with!` merge into one `retry!` that takes its
  policy as data, as Effect's `Effect.retry(effect, schedule)`, the
  backoff crate's `retry(backoff, op)` and Python's tenacity do.
  `Backoff { attempts: 5 }` is the common call, and an immediate retry is
  `initial: 0ms`. The price is `Clock` in every call's row (question 1).
- Removed: `all_list!`. A list of closures is awkward to build, and
  `map_all!(items, items.len(), work)` runs every input at once, as
  Effect's `forEach` does with unbounded concurrency. It stays as
  `map_all!`'s private engine.
- `factor` becomes a `u32`, since a negative factor is no policy.
- `TimedOut` stays a data type, as tokio's `Elapsed` is: the module
  fails in one way.
- Not proposed: jitter, which would put `Random` in the row, and a
  fail-fast `map_all!` like Go's `errgroup`; a deploy that stops at the
  first failure is a loop with `?`.

**Cost.** `retry!` allocates nothing per attempt beyond the attempt's own
frame, and `timeout!` is one race and one timer. `map_all!` fills one
list of slots, then copies it into a second. Under it, `all_list!` nests
one `all!` frame per worker, so with `limit` equal to n items each round
of polls is O(n²); the flat `all_frame` intrinsic already exists.

## std.testing

**Verdict: meets the bar, with one fixed combination.** Table tests,
property tests with shrinking and derived `Arbitrary`, inline and file
snapshots, per-case temporary directories and integration runs cover
what Go's `testing`, Rust's proptest and insta, and pytest give.

```text
pub fn hd_run!(command: Command) -> ProcessOutput $ Process       # was (name, args, stdin) -> RunOutput

impl Choices:
    pub fn int[N < Integer](mut self, range: Range[N]) -> N        # was (lo, hi)
    pub fn float[F < Float](mut self, range: Range[F]) -> F        # was (lo, hi)
```

- `hd_run!` takes job 1's `Command`, as Rust's `assert_cmd` builds a
  `std::process::Command` from `cargo_bin`, so a test sets arguments,
  stdin, environment and directory as a program does. Any exit status is
  data, and a program that does not start panics, as today.
- Removed: `RunOutput`, which was `ProcessOutput` under a second name.
- `Choices.int` takes a range, as `Rng.int` does after the data group's
  generic `int`, so `c.int(1..=6)` reads the same in a generator and in a
  program.
- Later: `TestRunner.report_timeout(millis: i64)` should take a
  `Duration`, and `TestRunner.temp_dir()` return a `Path`. Only std calls
  the trait, so the host change can wait for a language-tier batch.

**Cost.** A draw records one integer through the runner. Today `wide`
converts each drawn integer to `i64` by printing and parsing it, which
the data group's `Integer` conversions remove. `Choices.string` joins a
list of one-character strings where `push_char` would do.

## std.structure, std.annotation and std.inspect

**Verdict: the protocol meets the bar; one sentinel breaks principle 8.**
`Walker`, `Describer` and `Source` play the roles of serde's
`Serializer`, a schema walk and `Deserializer`, and every typed derive is
plain hd over them. The facts and the downcasts need no change.

```text
pub trait Source[S]:
    fn next(mut self, members: Members[S]) -> Result[Key[S]?, Self::Error]   # .None ends; was an end key

impl[HdS] Members[HdS]:
    pub fn at(self, position: usize) -> Key[HdS]?                            # was i32, giving the end key
    pub fn find(self, matches: fn(Member) -> bool) -> Key[HdS]?              # was the end key on no match
```

- Today the end is a `Key` with an `is_end` flag and a made-up `Member`
  at position -1. An optional key shows the end in the type, removes
  `Members.end()`, and lets `Member.position` and `VariantInfo.index` be
  `usize`, like every other index in std (question 2).

**Cost.** A template reads a member's facts once per type. But
`Members.find` takes a predicate, so a format that reads keys by name
scans every member per key, O(m²) for a value with m members; serde's
derive matches the name in generated code. A per-type name index from the
compiler, behind a lookup by name, would remove it. `TypeId` equality
compares two strings on every `downcast_val`, where one integer per type
would do.

## std.ops, std.convert, std.function and std.resource

**Verdict: `ops`, `convert` and `function` meet the bar; `resource` adds
a layer.** The operator traits, the ranges and `Default` are Rust's
`std::ops` and `Default`. `From` is Rust's, and text goes through
`Parse`. `Tuple` is a sealed marker. None of these change a name.

`std.resource.ResourceError[E]` wraps each handle error as `Operation(E)`
or `Disposed`, so `stream.read!(n)?` does not convert into a function's
`NetError`. Go's `net.ErrClosed` puts "closed" inside the module's error.
Proposal: `NetError.Closed`, and the module removed (question 2).

**Cost.** Slicing a list with `xs[a..b]` copies by contract, and `view`
does not. Today the copy pushes one element at a time, where one bulk
copy of a known size would do. Each integer type's range iterator builds
a closure.

## The Prelude

**Verdict: nearly right.** Four names serve implementers only, and every
test file imports the same two names.

- `Poll`, `PollContext` and `Waker` serve hand-written suspensions, which
  only std writes, and `Hasher` serves hand-written `Hash` impls. Rust's
  2021 prelude has `Future` but not `Poll`, `Context`, `Waker` or
  `Hasher`. Proposal: these four leave the prelude, and `Suspend` stays,
  since `race!` takes it.
- Every `tests:` block starts with `use std.testing.{assert,
  assert_equal}`, and the missing import is a row in the write-hd mistakes
  table. Rust's prelude has `assert!` and `assert_eq!`. Proposal: `assert`
  and `assert_equal` join `it` in the test prelude.

The prelude is language tier (question 2). A prelude name costs nothing
at run time.

## Structure Scenarios

Each program uses only the proposed surface of all three groups and
parses with `scripts/hd-conformance.sh parse`. None checks today.

**1. A link checker**: a breadth-first crawl with a queue and a seen set,
eight requests at a time, a timeout around retries of transient
failures, and a status summary.

```text
use std.collections.{Deque, Set, counts}
use std.http.{Client, Http, HttpError, Request}
use std.task.{Backoff, map_all, retry, timeout}
use std.time.{Clock, ms, s}
use std.error.Error

data Link:
    url: string
    status: u16 = 0                        # 0 when no response came back
    problem: string? = .None

fn transient(error: HttpError) -> bool:
    error.is_transient()

fn hrefs(page: string, site: string) -> List[string]:
    pieces := page.split("href=\"").iter().skip(1)
    paths := pieces.flat_map(fn(piece: string) -> string?: piece.split_once("\"").map(fn(pair: (string, string)) -> string: pair._0))
    paths.filter(fn(path: string) -> bool: path.starts_with("/")).map(fn(path: string) -> string: "$site$path").collect()

fn visit!(client: mut Client, url: string) -> (Link, string) $ Http + Clock:
    outcome := timeout!(20s):
        retry!(Backoff { attempts: 3, initial: 200ms }, when=transient):
            client.send!(Request::get(url))
    match outcome:
        .Ok(.Ok(response)) => (Link { url: url, status: response.status }, response.text())
        .Ok(.Err(.Status(response))) => (Link { url: url, status: response.status }, "")
        .Ok(.Err(error)) => (Link { url: url, problem: .Some("$error") }, "")
        .Err(late) => (Link { url: url, problem: .Some("$late") }, "")

pub fn crawl!(client: mut Client, site: string, limit: usize) -> List[Link] $ Http + Clock:
    let seen: mut Set[string] = Set::new()
    let queue: mut Deque[string] = Deque::new()
    queue.push_back("$site/")
    seen.insert("$site/")
    let found: mut List[Link] = []
    while !queue.is_empty() && found.len() < limit:
        let batch: mut List[string] = []
        while batch.len() < 32:
            match queue.pop_front():
                .Some(url) => batch.push(url)
                .None => break
        for (link, page) in map_all!(batch, 8, fn!(url: string) -> (Link, string) $ Http + Clock: visit!(client, url)):
            found.push(link)
            for next in hrefs(page, site):
                if seen.insert(next):
                    queue.push_back(next)
    found

pub fn main!() -> Result[void, dyn Error] $ Console + Http + Clock:
    let mut client = Client::new().header("User-Agent", "linkcheck/1.0")
    links := crawl!(client, "https://docs.example.com", 2000)
    for (status, count) in counts(links.map(fn(link: Link) -> u16: link.status)).most_common(5):
        println("$count  HTTP $status")
    broken := links.filter(fn(link: Link) -> bool: link.status >= 400 || link.problem.is_some())
    for link in broken.sorted_by_key(fn(link: Link) -> string: link.url):
        println("broken: ${link.url} ${link.problem.unwrap_or("HTTP ${link.status}")}")
    .Ok(())

```

**2. A log report**: typed lines with the bad ones dropped, a setting
layered from a flag, a variable and a default, an index built with
`get_or_insert`, and spikes found over sliding windows.

```text
use std.cli.{Cli, positional}
use std.collections.{Set, counts}
use std.fs.{FsRead, read_text}
use std.host.{Args, Env, env_as}
use std.path.Path
use std.error.{Error, message}

## Summarize an access log: who visits most, and where traffic spikes.
@derive(Cli)
data Report:
    ## Requests per minute that count as a spike; SPIKE_RPM when absent.
    spike: u32?
    ## How many users to list.
    top: usize = 5
    ## The log, one "minute user path millis" line per request.
    @positional
    log: Path

data Hit:
    minute: u32
    user: string
    path: string
    millis: u32

fn parse_hit(line: string) -> Hit?:
    fields := line.split(" ")
    if fields.len() != 4:
        return .None
    minute := fields[0].parse::[u32]().ok()?
    millis := fields[3].parse::[u32]().ok()?
    .Some(Hit { minute: minute, user: fields[1], path: fields[2], millis: millis })

pub fn main!() -> Result[void, dyn Error] $ Console + Args + Env + FsRead:
    report := Report::parse_args()?
    spike := usize(report.spike.or(env_as::[u32]("SPIKE_RPM")?).unwrap_or(600))
    hits := read_text!(report.log)?.lines().iter().flat_map(parse_hit).collect()
    if hits.is_empty():
        return .Err(message("no request lines in ${report.log}"))

    let paths: mut Map[string, mut Set[string]] = {}
    for hit in hits:
        paths.get_or_insert(hit.user, Set::new()).insert(hit.path)
    ranked := paths.entries().sorted_by_key(fn(entry: (string, mut Set[string])) -> usize: entry._1.len()).reversed()
    for (user, seen) in ranked.take(report.top):
        println("$user visited ${seen.len()} distinct paths")

    minutes := hits.map(fn(hit: Hit) -> u32: hit.minute)
    tally := counts(minutes)
    first := minutes.min().unwrap_or(0)
    rpm := [for minute in first..=minutes.max().unwrap_or(0) => tally.get(minute).unwrap_or(0)]
    for (index, window) in rpm.windows(5).iter().enumerate():
        total := window.iter().sum()
        if total > 5 * spike:
            println("spike: ${total / 5} requests a minute from minute ${first + u32(index)}")
    .Ok(())

```

**3. A command-line filter and its tests**: a table test, a property
test over a derived `Arbitrary` type, and an integration test that runs
the built program with an environment, a working directory and stdin.

```text
use std.console.{ConsoleInput, read_all}
use std.host.{Args, Env, args, env}
use std.error.{Error, message}

pub fn slug(title: string, separator: string) -> string:
    title.lower().split_whitespace().join(separator)

pub fn main!() -> Result[void, dyn Error] $ Console + ConsoleInput + Args + Env:
    if !args().is_empty():
        return .Err(message("slug takes no arguments; it reads standard input"))
    separator := env("SLUG_SEPARATOR").unwrap_or("-")
    for line in read_all!()?.lines():
        println(slug(line, separator))
    .Ok(())

tests:
    use std.testing.{Arbitrary, Choices, arbitrary, assert_equal, it_each, it_prop}

    @derive(Arbitrary, Debug)
    data Title:
        @arbitrary.with(fn(c: mut Choices) -> List[string]: c.list(4, fn(d: mut Choices) -> string: d.choose(["Tea", "Ünïcode", "a1", ""])))
        words: List[string]
        @arbitrary.with(fn(c: mut Choices) -> usize: c.int(1..=3))
        gap: usize

    it_each("slugs", [("Hello World", "hello-world"), ("  Trim  me ", "trim-me"), ("", "")], body=fn!(row: (string, string)):
        assert_equal(slug(row._0, "-"), row._1, reason="slug of ${row._0}")
    )

    it_prop("runs of spaces are one separator", prop=fn!(title: Title):
        tight := slug(title.words.join(" "), "-")
        loose := slug(title.words.join(" ".repeat(title.gap)), "-")
        assert_equal(loose, tight, reason="spacing never changes the slug")
    )
```

The integration test module, `tests/cli.hd`:

```text
use std.path.Path
use std.process.Command
use std.testing.{assert_equal, hd_run, snapshot}

it("reads stdin, honours the environment and the directory"):
    slug := Command::new("slug").env("SLUG_SEPARATOR", "_").current_dir(Path("tests/data")).stdin("Hello World\nTea Time\n")
    output := hd_run!(slug)
    assert_equal(output.status, +0, reason="success")
    snapshot(output.stdout, expect="hello_world\ntea_time\n")

it("reads a failure as data"):
    output := hd_run!(Command::new("slug").arg("--bogus"))
    assert_equal(output.status, +1, reason="an error exit")
    assert_equal(output.stderr.contains("takes no arguments"), true, reason="the message")
```

## Structure Public Names

Counted as in the other groups. Each `context` method is counted with its
file, `std.error`. "Before" is `lib/std` at `9c418be27`.

| Module | Before | After | Removed | Added |
| --- | --- | --- | --- | --- |
| `std.collections` | 72 | 72 | `Map.get_or` | `Map.get_or_insert` |
| `std.iter` | 23 | 23 | none | none (the `T?` impl is not a name) |
| `std.option` | 7 | 8 | none | `or` |
| `std.result` | 9 | 9 | none | none |
| `std.error` | 12 | 10 | free `chain`, `ContextError` (made private) | none |
| `std.task` | 9 | 7 | `retry_with!`, `all_list!` | none |
| `std.testing` | 33 | 32 | `RunOutput` | none |
| `std.structure` | 29 | 28 | `Members.end` | none |
| `std.ops` | 45 | 45 | none | none |
| `std.annotation`, `std.inspect` | 8 | 8 | none | none |
| `std.convert`, `std.function`, `std.resource` | 4 | 4 | none (question 2 would remove `ResourceError`) | none |
| **Total** | **251** | **246** | **7** | **2** |

The two added atoms each replace a nested `match` in real programs. The
removals are duplicates (`chain`, `get_or`, `RunOutput`), a merged pair
(`retry_with!`), an atom another covers (`all_list!`), a type no program
can read (`ContextError`), and a sentinel (`Members.end`).

## Across The Three Groups

The three groups agree on the main shapes. A builder is plain data with
bare-noun setters that take `mut self` (`Request`, `Client`, `Command`,
`ArgParser`), and its terminal is one bang call (`send!`, `run!`). A
policy is a literal with defaults (`Backoff`). Text reads into a type
through `Parse`, `Deserialize` or `Cli`, and the string-keyed layer stays
underneath. Every fake has one of four prefixes and is built with `new`,
and answering fakes share `reply` and `received`. Six places still break
a rule:

1. **`with`.** `with_x` copies a value with one part replaced
   (`Path.with_extension`), and a `_with` suffix takes a function
   (`it_prop_with`). `$.with` and `arbitrary.with` are language forms.
   `retry_with!` took a policy value and fit neither; it is merged above.
2. **Chaining.** `Request.header`, `Command.arg`, `ArgParser.arg` and
   `Sha256.update` return the value, but `StringBuilder.push` and
   `push_char` return `void`, though each feeds a value to one terminal
   (`build`, `finish`). Recommendation: one rule, "a value with a
   terminal call chains", so `push` returns `mut StringBuilder`, as Java's
   `append` does. Containers keep theirs (`List.push`, `Set.insert`).
   Statements are unaffected: only a `Result`, a `T?` or a suspension
   must be used.
3. **Parse error names.** `ParseNumberError`, `ParseBoolError` and
   `ParseCharError` name the operation first; `TimeParseError` does not.
   Recommendation: `ParseTimeError`, as Rust's `ParseIntError` reads.
4. **One error enum per module.** `std.host` will hold `EnvError` and the
   moved `SysError`; recommendation: one `HostError`. `ConsoleError` has
   only `Closed`, with no `NotGranted` or `Other`; recommendation: add
   both when `write!` lands. `ResourceError[E]` wraps instead of joining
   (question 2). Pure modules may name an error for the operation that
   several modules share (`DecodeError`, the `Parse` errors, `TimedOut`).
5. **Timeouts.** `connect_timeout!` goes because `timeout!` composes, but
   `Request.timeout` stays, though the same argument covers it.
   `Client.timeout`, a default for every call, is what `timeout!` cannot
   say, and `set_timeout` on a socket bounds each read. Recommendation:
   drop `Request.timeout` and keep the other two.
6. **Smaller rules.** Bounds are ranges (`Rng.int`, now `Choices.int`),
   sizes are `usize` (member positions and `Backoff.factor` are fixed
   above), spans are `Duration` (`report_timeout` waits), and each
   host-trait method has a free helper except `Sys` and the runner
   traits. Each exception left is named above.

| Group | Before | After |
| --- | --- | --- |
| I/O and host | 204 | 246 |
| Data | 239 | 247 |
| Structures and control | 251 | 246 |
| **Total** | **694** | **739** |

The "before" counts use three commits; `lib/std` changed between them by
one line, which moved no name. The whole library grows by 45 names, 6%.
The growth is in HTTP, processes, the typed command line and digests,
where the old surface could not express a realistic program; elsewhere
the count falls.

## Structure Questions For The Owner

1. **One `retry!`.** The merge puts `Clock` in the row of every retry,
   an immediate one included, so its test supplies a `ManualClock`.
   Keeping both keeps a retry for code with no clock, under a name that
   breaks the `with` rule. Recommendation: merge, as Effect's `retry`
   takes one schedule; retrying I/O at once is rarely right.
2. **Language-tier changes in this group.** (a) `Poll`, `PollContext`,
   `Waker` and `Hasher` leave the prelude, and `assert` and
   `assert_equal` join the test prelude. (b) `Source.next`,
   `Members.at` and `Members.find` return `Key?`, and member positions
   become `usize`. (c) A `Closed` variant in each handle module's error
   replaces `std.resource.ResourceError`. Recommendation: approve all
   three; (b) touches each derive template once, all in std.
3. **One word for a cause.** The derive marker is `@source`, while the
   method is `cause()`, the report field `causes`, and the printed line
   `caused by:`. Recommendation: rename the marker to `@cause`; adopting
   Rust's `source` would rename four things instead of one.
4. **The cross-module fixes.** Apply items 2 to 5 of the pass above,
   which touch the I/O and data proposals. Recommendation: apply all; each
   removes an exception to a rule the other groups already follow.
