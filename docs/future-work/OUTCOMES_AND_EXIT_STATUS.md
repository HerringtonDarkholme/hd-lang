# Outcomes And Exit Status: Design Study

Design exploration, 2026-10-17; nothing here is decided or in the
specification. It studies two open owner questions from #350. Should
`Request.send!` and `Command.run!` in the
[Standard Library Redesign](STDLIB_REDESIGN.md) fail a non-success
outcome by default? How does an hd error choose the exit status? It reads
[Entry Point](../../spec/lang/10-modules.md#entry-point),
[Exit Status](../../spec/lang/10-modules.md#exit-status),
[Error Trait](../../spec/lang/09-traits.md#error-trait),
[std.error](../../spec/std/error.md), [std.http](../../spec/std/http.md)
and [std.process](../../spec/std/process.md). Names such as `ok_status`
and `exit_code` are placeholders.

- [1. Problem, And What hd Has Today](#1-problem-and-what-hd-has-today)
- [2. Survey: Non-Success Outcomes](#2-survey-non-success-outcomes)
- [3. Options For Outcomes](#3-options-for-outcomes)
- [4. Comparison: Outcomes](#4-comparison-outcomes)
- [5. Survey: Exit Status](#5-survey-exit-status)
- [6. Options For Exit Status](#6-options-for-exit-status)
- [7. Comparison: Exit Status](#7-comparison-exit-status)
- [8. Security, Tests And Cost](#8-security-tests-and-cost)
- [Recommendation](#recommendation)
- [Questions For The Owner](#questions-for-the-owner)
- [Sources](#sources)
- [Parse Log](#parse-log)

**Decided by the owner, 2026-10-17:** (1) A5: `send!` fails for status 400 to 599 and `run!` for any non-zero exit, the error carries the whole response or output, and a builder setter (placeholder name `ok_status`) accepts an expected status. (2) B1: `Error` gains `exit_code()` with default 1 and `Termination` reads it; `CliError.Help` reports 0 on stdout, every other `CliError` 2. (3) An error may choose exit status 0, with the test-runner guard: a test whose result holds `.Err` fails. (4) std adds `impl Error for string`.

## 1. Problem, And What hd Has Today

### Question A: Is A Non-Success Outcome An Error?

An HTTP 404 and an exit status of 1 are both answers from a call that
worked. The redesign makes one terminal per builder, so that one call
must pick a default. The owner did not decide and asked for more
research.

Today the spec is split. `send!` and `Process.run!` return every outcome
as `.Ok`, with `Response.ok()` as the opt-in check. The helpers
`get_text!` and `output!` fail a non-success outcome. The redesign drops
the helpers and proposes that `send!` and `run!` fail by default, with
the whole response or output in the error.

Six cases stay fixed for every option:

| Case | What the program does |
| --- | --- |
| fetch | GET a JSON API and decode the body into `User` |
| exists | GET a URL where 404 is an expected answer that means "no" |
| retry | retry a request that may get 503, using `HttpError.is_transient` |
| git | run `git rev-parse HEAD` and use its output |
| grep | run `grep -q`, where status 1 means "no match", not failure |
| api-error | read the JSON error body of a 422 to show the user why |

### Question B: How Does An Error Choose The Exit Status?

`main` may return `void`, `ExitCode` or `Result[T, E]` with
`E < Display`. For `.Err(e)`, `Termination.report` returns 1, and the
host prints `e` and its causes on standard error. So the redesign's
`Pack::parse_args()?` prints `--help` on standard error with status 1.
clap, `argparse` and Go's `flag` exit 0. The owner chose to fix this
now, without special-casing `CliError` in the compiler.

Five cases stay fixed for every option:

| Case | What should happen |
| --- | --- |
| help | `pack --help` prints the usage on standard output and exits 0 |
| usage | `pack --levl 3` prints the error on standard error and exits 2, as clap and `argparse` do |
| own codes | a `diff`-like tool exits 0 for same, 1 for different, 2 for trouble |
| wrapped | the error passed through `?` into `dyn Error`, or through `context` |
| test | a unit test calls `Pack::parse(["--help"])` |

Neither question depends on an open core decision. The two interact in
one place, noted in [option B1](#b1-the-error-chooses-its-exit-code).

## 2. Survey: Non-Success Outcomes

HTTP clients:

| Library | Default for a 404 or 500 | Opt-in or escape | What the error keeps |
| --- | --- | --- | --- |
| Rust reqwest | `.Ok` response | `error_for_status()`, 4xx and 5xx [1] | status and URL; no body accessor [2] |
| Rust ureq 3 | `Err(StatusCode(u16))` for 4xx and 5xx | `http_status_as_error(false)` [3] | the status only |
| Go net/http | response, no error: "A non-2xx status code doesn't cause an error" [4] | read `StatusCode` | not an error |
| `fetch` | resolves: it "does not reject" on 404 or 504 [5] | read `ok` | not an error |
| ky (JS) | throws `HTTPError` for non-2xx, after redirects [6] | `throwHttpErrors: false` | `error.response` and parsed `error.data` |
| axios (JS) | rejects outside 200 to 299 | `validateStatus` predicate [7] | `error.response` |
| Python requests | response | `raise_for_status()`, 400 to 599 [8] | the response |
| Python urllib | raises `HTTPError`, which is also a readable response [9] | catch it | the response |
| Kotlin Ktor 2 | response; 2.0 changed `expectSuccess` to `false` [10] | `expectSuccess = true` | the response |
| .NET HttpClient | `GetAsync`: response; `GetStringAsync`: throws outside 200 to 299 [11] | `EnsureSuccessStatusCode()` | status |
| Elm http | `expectJson` gives `BadStatus code` [12] | `expectStringResponse` sees `BadStatus_ metadata body` | status (body in the raw form) |
| curl | exit 0 on 404, body saved | `--fail`, `--fail-with-body`, 400 and up [13] | body with `--fail-with-body` |

Process runners:

| Library | Default for a non-zero exit | Opt-in or escape | What the error keeps |
| --- | --- | --- | --- |
| Rust `Command` | `Ok(Output)` | `status.success()`; `exit_ok()` is unstable [14] | not an error |
| Go os/exec | `Run` and `Output` return `*ExitError` [15] | match the error | status; `Output` fills `Stderr` |
| Python subprocess | `run` returns; `check_output` raises | `check=True` raises `CalledProcessError` [16] | status, stdout, stderr |
| Node child_process | `execSync` throws; `spawnSync` returns [17] | use `spawnSync` | status, output |
| Deno `Command` | `output()` with a `success` flag [18] | read `success` | not an error |
| zx, Bun shell | throws | `.nothrow()` [19] [20] | the whole output |
| execa | rejects | `reject: false` [21] | the whole output |
| Haskell typed-process | `readProcess` returns the code | `readProcess_` throws `ExitCodeException` [22] | code, stdout, stderr |
| bash | ignored | `set -e` | nothing |

Takeaways:

1. **The split follows the caller.** General clients (reqwest, Go
   net/http, `fetch`, requests, Rust `Command`, Deno) leave the check to
   the caller. Tools for scripts (ky, axios, zx, execa, Bun, Go os/exec,
   curl's `--fail`) fail by default. ky lists it as its third benefit
   over `fetch` [6]. Ktor went the other way in 2.0, and its guide gives
   no reason [10].
2. **The "give me the data" call fails.** Where one library has both, the
   general call returns the outcome and the data call fails. Examples are
   .NET `GetStringAsync`, Python `check_output`, typed-process
   `readProcess_`, Elm `expectJson`, and ky's body methods. hd's redesign
   has one terminal by principle 5, so it cannot split this way.
3. **The boundary differs.** requests, reqwest, ureq and curl fail on 400
   and up. ky, axios, .NET and Ktor fail on any non-2xx. A 3xx reaches the
   caller only when it turned redirects off or sent a conditional request.
4. **A failing default keeps the response.** ky, axios, urllib, execa and
   zx put the whole response or output in the error, and ky's retry reads
   `Retry-After` from it [6]. reqwest's opt-in error keeps no body [2].

Two facts change the weight in hd. First, `send!` returns a `Result`
under every option, so the default only picks which arm a 404 lands in.
In ky or zx, a failing default is an exception that escapes a safe-looking
call; in hd it is one more `match` arm. Second, typed decoding catches
half the silent case: an HTML 404 fails `json::[User]()`, with a message
that hides the status.

## 3. Options For Outcomes

### A1. The Terminal Fails

`send!` returns `.Err(HttpError.Status(response))` for 400 to 599, and
`run!` returns `.Err(ProcessError.Exited(output))` for a non-zero exit.
This is the redesign's proposal, with the boundary moved from "outside
200 to 299" to "400 and up". The escape is a `match` on the error, which
holds everything the `.Ok` would have held.

```text
fn user!(id: u64) -> Result[User, HttpError] $ Http:
    Request::get("https://api.example.com/users/$id").send!()?.json::[User]()

fn exists!(url: string) -> Result[bool, HttpError] $ Http:
    match Request::get(url).send!():
        .Ok(_) => .Ok(true)
        .Err(HttpError.Status(response)) if response.status == 404 => .Ok(false)
        .Err(error) => .Err(error)

fn mentions!(path: string, word: string) -> Result[bool, ProcessError] $ Process:
    match Command::new("grep").arg("-q").arg(word).arg(path).run!():
        .Ok(_) => .Ok(true)
        .Err(ProcessError.Exited(output)) if output.status == 1 => .Ok(false)
        .Err(error) => .Err(error)
```

Retry composes with no extra code, because a 503 is an `.Err`:

```text
fn transient(error: HttpError) -> bool:
    error.is_transient()

fn fetch_report!(request: Request) -> Result[Response, HttpError] $ Http:
    retry!(3, when=transient):
        request.send!()
```

- Rules: stdlib tier only. `send!` and `run!` each gain one sentence on
  which outcomes are `.Err`. `Response.ok()` goes.
- Cases: fetch, git and retry need nothing. exists and grep need a
  guarded arm. api-error reads `response.json::[ApiError]()` from the
  `Status` payload.
- A forgotten check cannot happen; a forgotten expected 404 is a loud
  `.Err` on the first run.

### A2. Every Exchange Succeeds (Radical Simplification)

`send!` is the provider's raw round trip, and `run!` returns every exit.
Each result type has one opt-in check, `ok()`, which returns the same
`Status` or `Exited` error. This is reqwest, `fetch`, Go net/http and
Rust `Command`. It is the smallest design: std holds no policy about
which statuses are bad.

```text
fn user!(id: u64) -> Result[User, HttpError] $ Http:
    Request::get("https://api.example.com/users/$id").send!()?.ok()?.json::[User]()

fn head!() -> Result[string, ProcessError] $ Process:
    output := Command::new("git").arg("rev-parse").arg("HEAD").run!()?   # forgot ok(): outside a repo, stdout is ""
    .Ok(output.stdout.trim())

fn exists!(url: string) -> Result[bool, HttpError] $ Http:
    response := Request::get(url).send!()?
    .Ok(response.status != 404)                                          # a 500 reads as "exists"
```

- Rules: stdlib tier. `Response.ok()` stays, and `ProcessOutput.ok()`
  joins it.
- Cases: exists and grep read `status` directly. fetch, git and retry
  each need `ok()`, and a missed one is silent. `retry!` with
  `request.send!()` alone never retries a 503.
- The bug class: `head!` returns `""` as a commit hash, a text endpoint
  returns an error page, and a JSON error body may fit `User`'s defaults.

### A3. The Reader Checks

`send!` returns every status as `.Ok`, but the methods that read the body
as data, `text()` and `json::[T]()`, fail with `Status(self)` for 400 to
599. A separate reader takes any status. This is Elm's `expectJson`,
Retrofit's `body()`, and ky's body methods.

```text
impl Response:
    pub fn text(self) -> Result[string, HttpError]                   # .Err(Status(self)) for 400 to 599
    pub fn json[T < Deserialize](self) -> Result[T, HttpError]       # the same check
    pub fn error_json[T < Deserialize](self) -> Result[T, HttpError] # reads the body of any status

fn exists!(url: string) -> Result[bool, HttpError] $ Http:
    response := Request::get(url).send!()?
    match response.status:
        404 => .Ok(false)
        _ => if response.status >= 400: .Err(HttpError.Status(response)) else: .Ok(true)
```

- Rules: stdlib tier. `text()` becomes a `Result`, and one reader is
  added.
- Cases: fetch is A1's line. exists classifies the status itself.
  api-error needs the second reader. `retry!` retries a 503 only when
  the attempt also decodes.
- It does not carry over to `std.process`, whose `stdout` is a field
  with no reader to check in. The two terminals lose their shared rule.

### A4. A Typed Outcome

`send!` returns `Result[Outcome, HttpError]`, where `Outcome` splits the
response by class. No `Response` comes out until the caller picks an
arm or calls `success()`, so the check cannot be skipped. This is the
shape of Elm's raw `Response` type, with `GoodStatus_` and `BadStatus_`.

```text
pub enum Outcome:
    Success(response: Response)           # 100 to 399
    Failure(response: Response)           # 400 to 599

impl Outcome:
    pub fn success(self) -> Result[Response, HttpError]

fn user!(id: u64) -> Result[User, HttpError] $ Http:
    Request::get("https://api.example.com/users/$id").send!()?.success()?.json::[User]()

fn exists!(url: string) -> Result[bool, HttpError] $ Http:
    match Request::get(url).send!()?:
        Outcome.Success(_) => .Ok(true)
        Outcome.Failure(response) if response.status == 404 => .Ok(false)
        Outcome.Failure(response) => .Err(HttpError.Status(response))
```

- Rules: stdlib tier. Two enums (`Outcome` and a process counterpart)
  and two `success()` methods.
- Cases: none can forget the check, but every call pays a second `?`,
  and retry needs `success()` inside the attempt.
- It is `Result` inside `Result`: a second success-or-failure type with
  the meaning `Result` already has.

### A5. The Builder Names Its Accepted Outcomes

A1's default, plus one builder setter that moves a named status to the
`.Ok` side for that request. This is axios's `validateStatus`, Python's
`check=`, execa's `reject`, ureq's `http_status_as_error`, and Ktor's
`expectSuccess`, narrowed to a list of statuses.

```text
impl Request:
    pub fn ok_status(mut self, status: u16) -> mut Request   # this status also returns .Ok

impl Command:
    pub fn ok_status(mut self, status: i32) -> mut Command

fn exists!(url: string) -> Result[bool, HttpError] $ Http:
    response := Request::get(url).ok_status(404).send!()?
    .Ok(response.status != 404)

fn mentions!(path: string, word: string) -> Result[bool, ProcessError] $ Process:
    output := Command::new("grep").arg("-q").arg(word).arg(path).ok_status(1).run!()?
    .Ok(output.status == 0)
```

- Rules: A1's, plus one list field and one setter per builder.
- Cases: as A1, but exists and grep need no guard. The expected status
  is written where the request is built.
- A list, not axios's predicate, so `ScriptedHttp.received()` can
  compare it. Two ways remain to accept a 404: the setter or a `match`.

## 4. Comparison: Outcomes

| | A1 terminal fails | A2 all succeed | A3 reader checks | A4 typed outcome | A5 accepted outcomes |
| --- | --- | --- | --- | --- | --- |
| fetch | `send!()?.json()` | needs `ok()?` | `send!()?.json()` | needs `success()?` | as A1 |
| exists | guarded arm | reads `status` | reads `status` | match on class | `ok_status(404)` |
| retry on 503 | works as written | needs `ok()` inside | needs decode inside | needs `success()` inside | as A1 |
| git | works | silent `""` if `ok()` forgotten | needs A1 or A2 | needs `success()?` | as A1 |
| grep | guarded arm | reads `status` | needs A1 or A2 | match on class | `ok_status(1)` |
| api-error | `json` from `Status` payload | `json` | second reader | `json` on `Failure` | as A1 |
| Forgotten check | impossible | silent wrong data | silent for `status` readers | impossible | impossible |
| Rules added (all stdlib tier) | two sentences | none; one `ok()` each | two readers, one changed type | two enums, two methods | A1 plus two setters |
| Same rule for HTTP and process | yes | yes | no | yes | yes |
| Agent-writability | high | low: the check is easy to skip | medium | medium: double `?` | high |
| Human readability | good; guards for expected codes | good; checks scattered | good | heavy | best for expected codes |
| Implementation cost | small | none | small | medium | small |
| Evolution | can add A5 later | moving to A1 later breaks callers | hard to extend to process | hard to remove | can drop the setter later |

## 5. Survey: Exit Status

| System | How a failure picks the code | How help exits | Source |
| --- | --- | --- | --- |
| Rust | `main() -> Result<T, E: Debug>` prints `Error: {e:?}` and exits 1; `ExitCode` or a custom `Termination` for others | clap's `parse()` calls `Error::exit()` | [23] [24] |
| Rust `process::exit` | any code, at once; "no destructors" run | | [25] |
| clap | `Error::exit_code()` is 0 when `use_stderr()` is false, else 2; only help and version use stdout | `parse()` prints and exits 0 | [26] |
| Go | `os.Exit(code)`; deferred functions do not run | `flag`'s `ExitOnError` calls `os.Exit(0)` for `ErrHelp`, else `os.Exit(2)` | [27] |
| Python | `SystemExit(code)` is an exception the interpreter turns into the status; an uncaught exception exits 1 | `argparse` prints help, then `sys.exit(0)`; errors exit 2 | [28] [29] |
| Swift ArgumentParser | `exitCode(for:)` maps an error: help to success, validation to `EX_USAGE` | `exit(withError:)` prints a clean exit on stdout, others on stderr | [30] |
| Node | set `process.exitCode`; `process.exit()` stops even pending work, and the docs advise against it | | [31] |
| sysexits.h | `EX_USAGE` (64) to `EX_CONFIG` (78); FreeBSD now calls it "deprecated" and "discouraged" | | [32] |

Takeaways:

1. **The error knows its code.** clap, Swift ArgumentParser and
   `SystemExit` keep it with the error; clap derives it from the stream.
2. **Exiting at once is common and costly.** Rust, Go and Node skip
   destructors, deferred calls or pending work.
3. **Usage errors exit 2** in clap, `argparse` and Go's `flag`. FreeBSD
   discourages the sysexits codes.

## 6. Options For Exit Status

### B1. The Error Chooses Its Exit Code

`Error` gains one member with a default, as `cause` has. `Termination`
for `Result` reads it, and the host prints a code-0 error on standard
output and any other on standard error. This is clap's `exit_code` and
`use_stderr` in one value, and Python's `SystemExit`.

```text
pub trait Error < Display & Inspectable:
    fn cause(self) -> dyn Error?: .None
    fn exit_code(self) -> ExitCode: ExitCode(1)                # new

impl[T < Termination, E < Error] Termination for Result[T, E]:  # bound was E < Display
    fn report(self) -> ExitCode:
        match self:
            .Ok(value) => value.report()
            .Err(error) => error.exit_code()

impl Error for CliError:
    fn exit_code(self) -> ExitCode:
        match self:
            CliError.Help(_) => ExitCode(0)                    # printed on standard output
            _ => ExitCode(2)
```

The program needs no handling at all:

```text
pub fn main() -> Result[void, dyn Error] $ Args + Console:
    pack := Pack::parse_args()?        # --help: usage on stdout, status 0; --levl: stderr, status 2
    build(pack)
```

- Rules, language tier: the member and its default; `Termination` reads
  it; the stream follows the code; a transparent error forwards it.
  Stdlib tier: `CliError`'s impl.
- Bound: `E < Display` becomes `E < Error`. `string` is not an `Error`
  today, so `Result[void, string]` entry points and test bodies need
  `impl Error for string` or a move to `message`.
- Cases: help and usage work through `?`, and through `dyn Error` by
  dynamic dispatch. `context` gives the default 1, so wrapped help exits
  1, as clap's does inside anyhow. own codes: an `@error` enum overrides
  `exit_code`.
- Interaction with question A: `ProcessError.Exited` could forward the
  child's status, as shells do. That would surprise a reader, so it
  keeps the default 1.

### B2. An Exit Call

`std.process` adds an exit call behind a host capability, and std.cli
adds a clap-style `parse_args_or_exit`. This is Go's `os.Exit`, Rust's
`process::exit`, clap's `Error::exit` and Swift's `exit(withError:)`.

```text
pub trait Exit:                                    # a host capability trait
    fn exit(mut self, code: ExitCode) -> never

pub fn main() -> Result[void, dyn Error] $ Args + Console + Exit:
    pack := Pack::parse_args_or_exit()             # prints, then exits 0 or 2
    build(pack)
```

- Rules, language tier: a host capability trait in the default profile,
  and what an exit does to `defer` and `Drop`. Stdlib tier:
  `parse_args_or_exit`.
- Cases: help and usage take one line, and own codes work anywhere. The
  row names `Exit`, so a library cannot exit unseen.
- Costs: as in Rust, Go and Node, `defer` and `Drop` would not run. A
  unit test cannot bind `Exit`, so only `hd_run!` tests the help path.

### B3. Main Picks The Code (Radical Simplification)

No language change. `CliError` gains one std method that prints itself
to the right stream and returns the code. The program matches the parse
result and returns an `ExitCode`. This is Go's `flag` under
`ContinueOnError` with `errors.Is(err, flag.ErrHelp)`, and Rust's
`ExitCode`.

```text
impl CliError:
    pub fn print(self) -> ExitCode $ Console     # help on stdout and 0; other errors on stderr and 2

pub fn main() -> Result[ExitCode, dyn Error] $ Args + Console:
    pack := match Pack::parse_args():
        .Ok(pack) => pack
        .Err(error) => return .Ok(error.print())
    build(pack)?
    .Ok(ExitCode(0))
```

- Rules: none in the language; one std method.
- Cases: help and usage cost four lines in every program. A program that
  writes `parse_args()?` still exits 1 on help, as today.
- It keeps the spec's stance: the result type, not the error, picks the
  code.

### B4. Help Is Not An Error

`CliError.Help` goes. `parse_args` prints the help itself and returns
`.Ok(.None)`, so the program stops with `.Ok(())` and status 0. This is
`argparse`, which prints the help and exits on its own, with the exit
replaced by a value.

```text
pub fn main() -> Result[void, dyn Error] $ Args + Console:
    pack := match Pack::parse_args()?:             # Result[Pack?, CliError]; help was printed
        .Some(pack) => pack
        .None => return .Ok(())
    build(pack)
```

- Rules: none in the language. `parse_args` changes its result to
  `Result[T?, CliError]` and its row to `$ Args + Console`.
- Cases: help works, and the type forces the program to handle it. usage
  still exits 1. The pure `parse` that tests call needs another way to
  return the help text.

## 7. Comparison: Exit Status

| | B1 error chooses | B2 exit call | B3 main picks | B4 help as value |
| --- | --- | --- | --- | --- |
| help (stdout, 0) | through `?` | one call | four lines | through a match |
| usage (stderr, 2) | through `?` | one call | four lines | exits 1 |
| own codes | `exit_code` on the error, or `ExitCode` | anywhere | `ExitCode` | `ExitCode` |
| wrapped by `context` | exits 1 | not reached | not reached | not reached |
| unit test of `--help` | must not pass by accident | needs `hd_run!` | as today | as today |
| `defer` and `Drop` | run | skipped | run | run |
| Can a dependency change the status? | yes, through its error type | only with `Exit` in a row | no | no |
| Rules added | four, language tier | a host trait, language tier | none | none |
| Agent-writability | high | high | low: four lines to remember | medium |
| Implementation cost | small: std, host printing, runner | large: host ABI and cleanup | tiny | small |
| Evolution | other errors can opt in later | hard to take back | can grow into B1 | fixed to std.cli |

## 8. Security, Tests And Cost

**An error that reports success.** Under B1, any error type, including
one from a dependency, can make `.Err` exit 0. A failed deploy script
would then look green to CI. hd's capability rows show what a call can
do, and an error value bypasses them. Python's `SystemExit(0)` and Rust's
`process::exit(0)` have the same power with no row. B1 could forbid code
0 for errors, but then help needs B3 or B4.

**The test runner.** A test passes today when `report()` on its result
returns 0. Under B1, a body that returns `.Err(CliError.Help(...))` would
pass. B1 therefore also needs one runner rule: a result that holds
`.Err` fails, whatever its code. The runner already detects `.Err` to
print it.

**Panics.** No option changes them; a panic keeps the profile's status.

**What an error prints.** Under A1 and A5, a failed request reaches
`main` through `?` and is printed, often into CI logs. reqwest warns
that its error "may include the full URL", and offers `without_url` [2]. So
`Status`'s text names the status and the URL without its query or
fragment, and never the body or headers. `Exited` names the program,
the status and the last line of stderr, never the arguments, which may
hold a token. Go's `ExitError` prints only `exit status N` [15].

**Cost.** A1 and A5 move the `Response` or `ProcessOutput` into the error
variant: no body copy, and one variant in place of the `.Ok`. A5's list
is empty unless set. B1 adds one dynamic call per failing `main`. B2
needs a host ABI addition and a defined cleanup order.

## Recommendation

This section is advice. The owner decides.

**Question A: A5, the terminal fails, and the builder names its accepted
outcomes.**

- `send!` fails 400 to 599, the boundary of requests, reqwest, ureq and
  curl. A 3xx arrives only when the caller asked for it.
- `run!` fails any non-zero status. Both errors carry the whole result.
- `ok_status(404)` and `ok_status(1)` accept expected answers in the line
  that builds the call.
- Why: under A2 a forgotten check is silent wrong data; under A5 a
  forgotten 404 is a loud `.Err` on the first run. Agents skip opt-in
  checks. In hd the failing default is a visible `Result` arm, not an
  escaping exception, and retry composes with no extra code.
- Gives up: one setter per builder, and two ways to accept a 404. Next
  best: A1 alone.

**Question B: B1, the error chooses its exit code, with two guards.**

- `Error` gains `exit_code()` with default 1, read by `Termination`. A
  code-0 error prints on standard output. `CliError.Help` reports 0, and
  every other `CliError` reports 2.
- Guard 1: the test runner fails any result that holds `.Err`.
- Guard 2: `string` implements `Error`, so `Result[_, string]` keeps
  checking under the `E < Error` bound.
- Why: `parse_args()?` is right with no handling, as clap's `parse()` is,
  and `defer` and `Drop` still run. One member serves every error type,
  so `CliError` is not a special case.
- Gives up: an error type can turn a failure into status 0, which no
  capability row shows. Next best: B3, no rule and four lines per
  program.

## Questions For The Owner

1. **Default for a non-success outcome.** When a program forgets to look
   at the status, it reads an error page or an empty stdout as data, or
   it fails loudly. Options: fail in the terminal (A1); fail in the
   terminal, plus a builder setter for expected statuses (A5); every
   exchange succeeds, check with `ok()` (A2). Recommended: A5, with 400
   to 599 and any non-zero exit as errors.

   ```text
   response := Request::get(url).ok_status(404).send!()?
   ```

2. **How an error chooses its code.** A help request either exits 0
   through `?` or needs code in every program. Options: an `Error` member
   with default 1 (B1); an exit call behind a host capability (B2);
   `main` returns `ExitCode` and std.cli gives `CliError.print()` (B3).
   Recommended: B1.

   ```text
   pack := Pack::parse_args()?   # B1: --help prints on stdout and exits 0
   ```

3. **May an error choose exit status 0?** If yes, `parse_args()?` makes
   `--help` exit 0, and any error type could make a failure look like
   success. Options: yes, with the test-runner guard; no, codes 1 to 255
   only, and help goes through B3 or B4. Recommended: yes.

   ```text
   impl Error for CliError:
       fn exit_code(self) -> ExitCode:
           if self.is_help(): ExitCode(0) else: ExitCode(2)
   ```

4. **String errors under the `E < Error` bound.** With B1, a `main` that
   returns `Result[void, string]` either keeps checking or must change.
   Options: std adds `impl Error for string`; such code moves to
   `dyn Error` with `message`. Recommended: `impl Error for string`.

   ```text
   pub fn main() -> Result[void, string]:
       .Err("no input")
   ```

## Sources

1. reqwest `Response::error_for_status`: https://docs.rs/reqwest/latest/reqwest/struct.Response.html
2. reqwest `Error`, its methods and the `without_url` note: https://docs.rs/reqwest/latest/reqwest/struct.Error.html
3. ureq `ConfigBuilder::http_status_as_error` (default `true`) and `Error::StatusCode`: https://docs.rs/ureq/latest/ureq/config/struct.ConfigBuilder.html
4. Go `net/http`, `Client.Do`: https://pkg.go.dev/net/http
5. MDN, `fetch()`: https://developer.mozilla.org/en-US/docs/Web/API/Window/fetch
6. ky README (benefits, `throwHttpErrors`, `HTTPError`, retry `statusCodes`): https://github.com/sindresorhus/ky
7. axios request config, `validateStatus`: https://axios-http.com/docs/req_config
8. requests `Response.raise_for_status`, source: https://github.com/psf/requests/blob/main/src/requests/models.py
9. Python `urllib.error.HTTPError`: https://docs.python.org/3/library/urllib.error.html
10. Ktor response validation and the 2.0 migration guide: https://ktor.io/docs/client-response-validation.html, https://ktor.io/docs/migration-to-20x.html
11. .NET `HttpClient.GetStringAsync`: https://learn.microsoft.com/en-us/dotnet/api/system.net.http.httpclient.getstringasync
12. Elm `Http`, `expectJson` and `Response`: https://github.com/elm/http/blob/2.0.0/src/Http.elm
13. curl manual, `--fail` and `--fail-with-body`: https://curl.se/docs/manpage.html
14. Rust `ExitStatus`, `success` and `exit_ok`: https://doc.rust-lang.org/std/process/struct.ExitStatus.html
15. Go `os/exec`, `Cmd.Run`, `Cmd.Output`, `ExitError`: https://pkg.go.dev/os/exec
16. Python `subprocess.run`, `check`: https://docs.python.org/3/library/subprocess.html
17. Node `child_process.execSync`: https://nodejs.org/api/child_process.html
18. Deno `CommandOutput.success`: https://docs.deno.com/api/deno/~/Deno.CommandOutput
19. zx `nothrow()`: https://github.com/google/zx/blob/main/docs/process-promise.md
20. Bun shell, `.nothrow()`: https://bun.sh/docs/runtime/shell
21. execa `reject` option: https://github.com/sindresorhus/execa/blob/main/docs/api.md
22. typed-process, `readProcess_` and `ExitCodeException`: https://hackage.haskell.org/package/typed-process/docs/System-Process-Typed.html
23. Rust `Termination` for `Result<T, E: Debug>`: https://doc.rust-lang.org/std/process/trait.Termination.html
24. Rust `ExitCode`: https://doc.rust-lang.org/std/process/struct.ExitCode.html
25. Rust `process::exit`: https://doc.rust-lang.org/std/process/fn.exit.html
26. clap `Error::use_stderr`, `exit_code`, `exit`, source: https://github.com/clap-rs/clap/blob/master/clap_builder/src/error/mod.rs
27. Go `flag.FlagSet.Parse`, `ExitOnError`, source: https://go.dev/src/flag/flag.go
28. Python `SystemExit`: https://docs.python.org/3/library/exceptions.html#SystemExit
29. Python `argparse` help action and `exit`, source: https://github.com/python/cpython/blob/main/Lib/argparse.py
30. Swift ArgumentParser, `exit(withError:)` and `MessageInfo.exitCode`: https://github.com/apple/swift-argument-parser/tree/main/Sources/ArgumentParser
31. Node `process.exit` and `process.exitCode`: https://nodejs.org/api/process.html
32. FreeBSD `sysexits(3)`: https://man.freebsd.org/cgi/man.cgi?query=sysexits

## Parse Log

Every `text` block was parsed with `scripts/hd-conformance.sh parse`.
All 15 blocks parse. Parsing checks syntax only, and no block was
type-checked. The blocks leave out their `use` lines, and names such as
`Request`, `Command`, `Pack` and `CliError` follow the redesign, not std
today. No block needed `# hypothetical syntax`.
