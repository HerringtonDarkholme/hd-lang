# Streaming: One Reader And Writer Shape

**Owner decisions (2026-10-17).** Handles cross the host boundary as
plain descriptors: a host method returns a plain id, and std wraps it in
an `own` handle whose drop calls the host's close, as Rust's `File` wraps
a file descriptor, so ownership slice 7's `not-boundary-safe` rule needs
no exception. The `Closed` variant in each handle module's error stays,
as the owner approved earlier (section 8 is overruled). Every question
in section 13 is accepted as recommended, and so is the block rule of
section 7, with the owner's change that a block capturing a `^mut`
borrow is a mutable borrowed closure (`^{c} mut fn!()`), callable any
number of times while the borrow lives: `timeout!`'s `work` and the
children of `race!` and `all!` take `own fn!()`, which a fresh block
satisfies whatever it captures; `retry!`'s attempt may capture a `^mut`
handle, since its calls run one at a time; `map_all!`'s `work` runs for
several items at once, so it captures only plain values and shared
borrows; and a plain `fn` is accepted wherever an `own fn` or a
borrowed `fn` is expected.

This is a proposal for issue #350, not a decision. The owner asked on
2026-10-17 to "design streaming now", before the I/O redesign is
implemented. It answers question 5 of
[Standard Library Redesign](STDLIB_REDESIGN.md): HTTP bodies, process
output and large files are buffered whole, and the recommendation there
was one reader shape for all three, added beside the buffered terminals.

The design builds on the decided ownership model of
[Ownership And Borrows](OWNERSHIP_DESIGN.md), now in
[Ownership and Borrows](../../spec/lang/13-ownership.md): `own` at the
use site, `^` borrows, `DropOwn` at scope end and on overwrite, the
`OwnOnly` marker, `for` that lends, and an explicit `finish!()` for
cleanup that suspends or can fail. Every name below is a placeholder.

## 1. The Problem

Three buffered terminals cover the common case and stay as they are:

```text
page := Request::get(url).send!()?          # the whole body in memory
output := Command::new("git").arg("log").run!()?   # all of stdout and stderr
notes := read_text!(Path("notes.md"))?      # the whole file
```

Four programs do not fit them: a download larger than memory or with
progress, a build that prints for minutes, a log of several gigabytes
read line by line, and a line protocol such as SMTP over a socket.
`TcpStream` already streams, with `read!(max)` and `write!(bytes)`; the
other sources have nothing. The aim is one shape for all four, so a
helper written once (count lines, copy, hash) serves every source.

## 2. The Design On One Page

A new module `std.io` holds two traits, one line producer, one copy
function and their errors. Each source module adds one streaming
terminal beside its buffered one. The terminal returns an owned handle,
which implements `Reader`, `Writer` or both, and reports the source
module's own error.

```text
pub trait ErrorType:
    type Error                                   # implements std.error.Error

pub trait Reader < ErrorType:
    fn read_into!(^mut self, buffer: ^mut List[u8], max: usize) -> Result[usize, Self::Error]
    fn read!(^mut self, max: usize) -> Result[List[u8], Self::Error]     # default: a new list, then read_into!
    fn read_all!(^mut self) -> Result[List[u8], Self::Error]            # default: read_into! until the end

pub trait Writer < ErrorType:
    fn write!(^mut self, bytes: ^List[u8]) -> Result[void, Self::Error]
    fn write_text!(^mut self, text: string) -> Result[void, Self::Error]  # default: write! of the UTF-8 bytes
    fn flush!(^mut self) -> Result[void, Self::Error]                    # default: .Ok(())
    fn finish!(own self) -> Result[void, Self::Error]                    # default: flush!, then drop
```

| Source | Buffered terminal (unchanged) | Streaming terminal (new) | Handle | Error |
| --- | --- | --- | --- | --- |
| HTTP | `Request.send!`, `Client.send!` | `send_streaming!` | `own StreamedResponse`: `Reader` | `HttpError` |
| Process | `Command.run!` | `Command.spawn!` | `own Child` with `own PipeReader` and `own PipeWriter` | `ProcessError` |
| Files | `read_text!`, `write_text!` | `open!`, `create!` | `own FileReader`, `own FileWriter` | `FsError` |
| TCP | none | `connect!`, `accept!` | `own dyn TcpStream`: both | `NetError` |

1. **Pull, not push.** Nothing is read until the program reads. A read
   returns at least one byte, or zero bytes at the end.
2. **Owned handles.** Every handle is `OwnOnly` and `DropOwn`. Dropping
   it closes it, on every exit and on cancellation. There is no `close`
   and no `Closed` error.
3. **Borrowed use.** Reads and writes take `^mut self`, so a helper takes
   `^mut R` or `^mut dyn Reader[Error = E]` and the caller keeps the
   handle.
4. **Module errors.** A handle reports its module's enum through the
   associated type `Error`.
5. **`finish!` reports.** A writer whose close can fail is finished with
   the consuming `finish!()`. A dropped writer closes silently.

## 3. A Tour

### 3.1 Reading A Body Chunk By Chunk

A downloader shows progress while it hashes a large file:

```text
use std.digest.Sha256
use std.http.{Http, Request}

fn fetch_digest!(url: string) -> Result[string, dyn Error] $ Http + Console:
    let own response = Request::get(url).send_streaming!()?
    total := response.header("content-length").unwrap_or("?")
    let hasher: mut Sha256 = Sha256::new()
    let buffer: mut List[u8] = []
    let seen: u64 = 0
    while true:
        buffer.clear()
        n := response.read_into!(buffer, 65536)?
        if n == 0:
            break
        hasher.update(buffer)
        seen = seen + u64(n)
        println("$seen of $total bytes")
    .Ok(hasher.finish().to_hex())
```

`send_streaming!` returns once the status and headers arrive; they are
plain fields, as on `Response`. The response itself implements `Reader`
with `Error = HttpError`. When `fetch_digest!` returns, by any path, the
response drops and a body not read to its end is aborted, as `fetch`
aborts through an `AbortController`. The buffer keeps its capacity
across `clear()`, so the loop allocates once. A body cut short of its
`Content-Length` is an error, never an early end, so a truncated
download cannot pass as complete.

### 3.2 Lines From A Child Process

A build runner prints each warning while `cargo` runs:

```text
use std.io.LineReader
use std.process.{Command, Process, Stdio}

fn build!(dir: Path) -> Result[usize, dyn Error] $ Process + Console:
    let own child = Command::new("cargo").arg("build").current_dir(dir).stdout(Stdio.Inherit).spawn!()?
    let own err = match child.stderr.take():
        .Some(pipe) => pipe
        .None => panic("stderr is piped by default")
    let own lines = LineReader::new(err)
    let warnings: usize = 0
    while true:
        line := match lines.read_line!()?:
            .Some(text) => text
            .None => break
        if line.starts_with("warning"):
            warnings = warnings + 1
            println(line)
    child.wait!()?
    .Ok(warnings)
```

`spawn!` returns an owned `Child` whose pipes are `Option[own ...]`
fields; the program moves out the ones it reads with `take()`, the
decided way to move an owner out of a field. `LineReader::new(err)`
borrows the pipe mutably, so the reader is `OwnOnly` named data driven
by `while`.

`wait!(own self)` first closes a stdin pipe the child still holds, so a
child that reads to the end of its input finishes. It reads any pipe
still held to its end into the `ProcessOutput`, and a non-zero exit is
`.Err(ProcessError.Exited(output))`, as `run!` reports it. Dropping a
`Child` not waited for kills its process group, as cancelling `run!`
does, so a cancelled build or an early `?` leaves no orphan.

```text
pub data Child:
    pub stdin: own PipeWriter?
    pub stdout: own PipeReader?
    pub stderr: own PipeReader?

impl OwnOnly for Child
impl DropOwn for Child:
    fn drop(^mut self) -> void                    # kills the process group if still running

impl Child:
    pub fn wait!(own self) -> Result[ProcessOutput, ProcessError]

impl Command:
    pub fn spawn!(self) -> Result[own Child, ProcessError] $ Process
    pub fn stdout(mut self, mode: Stdio) -> mut Command
    pub fn stderr(mut self, mode: Stdio) -> mut Command

pub enum Stdio:
    Piped                                         # the default; run! captures it
    Inherit                                       # the child writes to the program's own stream
    Null
```

A child that fills a pipe nobody reads blocks, as in Rust and Go, which
is why `build!` inherits stdout: a program reads both pipes with `all!`
or inherits the one it does not read. `Stdio` serves `run!` too, where
an inherited stream is `""` in the output.

### 3.3 Copying A Download To A File

```text
use std.fs.{FsWrite, create}
use std.io.copy

fn download!(url: string, to: Path) -> Result[u64, dyn Error] $ Http + FsWrite:
    let own response = Request::get(url).send_streaming!()?
    let own file = create!(to)?
    copied := copy!(response, file)?
    file.finish!()?
    .Ok(copied)
```

`copy!` lends both handles, copies through one buffer until the reader
ends, and returns the byte count. It does not finish the writer, as Go's
`io.Copy` does not close one. `finish!()` flushes and closes the file,
and reports a full disk or a failed close. If `copy!` fails, both
handles drop: the request is aborted and the file keeps what was
written. A program that must not leave a partial file writes to a
temporary path and calls `rename!` after `finish!`.

```text
pub enum CopyError[R, W]:
    Read(error: R)
    Write(error: W)

pub fn copy![R < own Reader, W < own Writer](from: ^mut R, to: ^mut W) -> Result[u64, CopyError[R::Error, W::Error]]:
    let buffer: mut List[u8] = []
    let total: u64 = 0
    while true:
        buffer.clear()
        n := match from.read_into!(buffer, 65536):
            .Ok(count) => count
            .Err(error) => return .Err(.Read(error))
        if n == 0:
            break
        match to.write!(buffer):
            .Ok(_) => pass
            .Err(error) => return .Err(.Write(error))
        total = total + u64(n)
    .Ok(total)
```

`CopyError` says which side failed, which Go's `io.Copy` cannot: its one
`error` may come from either end.

### 3.4 A Line Protocol Over TcpStream

A cache client sends a command and reads one reply line:

```text
use std.io.LineReader
use std.net.{Net, NetError, connect}

fn lookup!(key: string) -> Result[string?, dyn Error] $ Net:
    let own stream = connect!("cache.local", 6379)?
    let own lines = LineReader::new(stream)
    lines.source().write_text!("GET $key\n")?
    match lines.read_line!()?:
        .Some(reply) => .Ok(reply.strip_prefix("VALUE "))
        .None => .Ok(.None)
```

The line reader holds the stream's only `^mut` borrow, so the program
writes through `lines.source()`, which reborrows the stream, as Rust's
`BufReader::get_mut` does. The reader keeps its buffered bytes across
the write. When `lookup!` returns, `lines` drops first, then `stream`,
which closes the socket.

`TcpStream` becomes:

```text
pub trait TcpStream < Reader[Error = NetError] & Writer[Error = NetError] & DropOwn:
    fn set_timeout(^mut self, timeout: Duration?) -> Result[void, NetError]
    fn set_nodelay(^mut self, enabled: bool) -> Result[void, NetError]
    fn set_keepalive(^mut self, idle: Duration?) -> Result[void, NetError]
    fn close_write!(^mut self) -> Result[void, NetError]

pub trait TcpListener < DropOwn:
    fn accept!(^mut self) -> Result[own dyn TcpStream, NetError]

pub fn connect!(host: string, port: u16) -> Result[own dyn TcpStream, NetError] $ Net: panic("stub")
```

`read!` and `write!` move to `Reader` and `Writer`, and `close` goes.
`close_write!` tells the peer the request is complete while the program
still reads the reply, as Go's `TCPConn.CloseWrite` does. `finish!` on a
stream is `close_write!` followed by the drop.

### 3.5 Writing To A Child

A program pipes generated text into `sort` and reads the result:

```text
use std.task.all

fn sorted!(lines: List[string]) -> Result[string, dyn Error] $ Process:
    let own child = Command::new("sort").spawn!()?
    let own input = child.stdin.take().unwrap()
    let own output = child.stdout.take().unwrap()
    let (written, text) = all!(feed(input, lines), output.read_all())
    written?
    child.wait!()?
    .Ok(string::from_utf8(text?)?)

fn feed!(input: own PipeWriter, lines: List[string]) -> Result[void, ProcessError]:
    for line in lines:
        input.write_text!("$line\n")?
    input.finish!()
```

`feed!` owns the writer and finishes it, which closes the child's
stdin, so `sort` sees the end of its input. The two children of `all!`
run together, so neither pipe can fill while the other waits, and
`all!` returns both results. If the caller is cancelled, the frames
drop, the pipes close, and the child is killed when `child` drops.

## 4. Line And Chunk Iteration

Chunk iteration needs no type: a `while` loop over `read_into!`, as in
3.1. Lines need a buffer that outlives each call, so they get a named
producer:

```text
pub data LineReader[R, ^r]:
    source: ^{r} mut R
    buffer: mut List[u8]                          # bytes read but not yet returned
    limit: usize = 1048576

impl[R < own Reader, ^r] OwnOnly for LineReader[R, ^{r}]

impl[R < own Reader, ^r] LineReader[R, ^{r}]:
    pub fn new(source: ^{r} mut R) -> own LineReader[R, ^{r}]
    pub fn limit(own self, max: usize) -> own LineReader[R, ^{r}]
    pub fn read_line!(^mut self) -> Result[string?, LineError[R::Error]]
    pub fn read_until!(^mut self, byte: u8) -> Result[List[u8]?, LineError[R::Error]]
    pub fn source(^mut self) -> ^mut R

pub enum LineError[E]:
    Read(error: E)
    TooLong(limit: usize)
    InvalidUtf8(bytes: List[u8])
```

- `read_line!` returns the next line without its `"\n"` or a `"\r"`
  before it, as Go's `ScanLines` and Rust's `lines()` do, including a
  last line with no newline. `.None` is the end.
- A line over `limit` bytes is `.Err(LineError.TooLong(limit))`, as Go's
  `bufio.Scanner` reports `ErrTooLong`, so a peer that never sends a
  newline cannot exhaust memory.
- A line that is not UTF-8 is `.Err(LineError.InvalidUtf8(bytes))`, with
  the bytes kept. `read_until!` returns raw bytes for formats that are
  not text.
- `LineReader` implements `Reader` too, serving its buffered bytes
  first, so a parser reads headers by line and a body as bytes. It
  refills 8 KiB at a time, as Rust's `BufReader` does by default.

`for` cannot drive `read_line!`, because `for` takes an `Iterator`,
whose `next` cannot suspend. The `while` and `match` form is four lines
and matches `read_line!` on the console. tokio's `Lines::next_line`,
driven by `while let`, has the same shape.

## 5. Backpressure

Every handle is pulled. The host reads from the operating system only
when the program calls a read, and keeps no queue beyond the bytes of
one read that the program has not taken. A slow consumer therefore
slows the producer: TCP's receive window closes, a child blocks on a
full pipe, and an HTTP body is read from the network only as fast as
the program reads it. `write!` completes only once the host has
accepted every byte, so a fast producer waits for a slow peer.

This is Go's and Rust's model, and it needs no settings, where Node
needs a `highWaterMark` and a `'drain'` event and WHATWG streams a
queuing strategy. Suspension already makes waiting cheap, so a queue
would add a setting and remove no problem.

## 6. Cancellation And Drop

The decided rules settle most of this:

- **The owner is cancelled.** Its owned handles drop, in reverse order
  of declaration. A socket closes, a file closes, an HTTP request is
  aborted, and a child's process group is killed. A writer is not
  finished: nothing is flushed over the network, as the ownership study
  decides.
- **A read is cancelled.** It returns nothing, and the bytes the host
  had not returned stay for the next read on that handle, as the net
  spec's rule for `read!` already says, extended to every `Reader`.
  tokio documents the same cancel safety for `read`.
- **A line read is cancelled.** The partial line stays in the
  `LineReader`'s buffer, which its owner holds, so the next
  `read_line!` returns the whole line.
- **A write is cancelled.** An unknown number of bytes went out. The
  handle stays usable, but its position in the protocol is unknown, so
  a framed protocol drops it.
- **A copy is cancelled.** Bytes read but not yet written are lost, as
  with tokio's `copy`.
- **A handle dropped during an operation** cannot happen: the
  operation borrows the handle, and the borrow ends before any drop. The
  net spec's rule for a pending operation on a closed stream goes.

A file writer's host may keep a buffer. Its drop asks the host to close
synchronously, which flushes and ignores errors, as Rust's `BufWriter`
and `File` do on drop. `finish!` is the path that reports them. A
network writer keeps no buffer in std, so a dropped one loses nothing
that `write!` acknowledged.

## 7. Timeouts

```text
use std.task.timeout

fn first_line!(host: string) -> Result[string?, dyn Error] $ Net + Clock:
    let own stream = connect!(host, 25)?
    stream.set_timeout(.Some(5s))?                # each later read waits at most 5s
    let own lines = LineReader::new(stream)
    attempt := timeout!(30s):                     # the whole exchange, one timer
        lines.read_line!()
    .Ok(attempt??)
```

Two bounds serve two needs. `timeout!` around a loop bounds the whole
transfer with one timer and one race. `set_timeout` on a handle bounds
each read, for a peer that goes quiet; it stays on `TcpStream` because a
`timeout!` around every read would pay one timer and one race per read.
HTTP bodies, pipes and files take `timeout!` around the loop. An idle
bound on them waits for a program that needs one.

A `timeout!` block that names an owned handle moves it in, as `lines`
above; one that names a `^mut` borrow captures it exclusively. Either
way the block is a one-shot closure under the decided rules, so
`timeout!`'s `work` parameter becomes `own fn!() -> T $ R`, which a
fresh trailing block satisfies. To keep a handle after the block, to
`finish!` a file for example, bind a borrow first
(`let out: ^mut FileWriter = file`) and name that. `map_all!` and `race!`
children need the same check; `retry!` calls its attempt more than once,
so an attempt cannot borrow a handle mutably, and retrying a connect
needs none.

## 8. Errors

Each handle reports its module's enum, as the shared convention
requires: `HttpError`, `ProcessError`, `FsError` or `NetError`. The
associated type `Error` carries it, as the embedded-io crate's
`ErrorType` does, so a generic helper writes `R::Error`. `std.io` names
errors for its own operations only, as the convention allows a pure
module (`DecodeError`, `TimedOut`): `LineError[E]` and `CopyError[R, W]`.
A program that mixes sources returns `dyn Error`, as the tour does, or
its own enum with `From` impls, which `?` finds.

The owner approved a `Closed` variant in each handle module's error to
replace `std.resource.ResourceError`. With owned handles it cannot
occur, since a call after a drop or a move is `use-after-move`. So
`Closed` is not added, and `ResourceError` goes as decided.
`NetError.Timeout` stays for `set_timeout`.

## 9. Test Fakes

`std.io` adds two fakes, and each scripted provider serves its new
terminal:

```text
impl ScriptedReader:
    pub fn new(chunks: List[List[u8]]) -> own ScriptedReader[E]
    pub fn failing(chunks: List[List[u8]], error: E) -> own ScriptedReader[E]

impl ScriptedWriter:
    pub fn new() -> (own ScriptedWriter[E], WriteLog)
    pub fn failing(after: usize, error: E) -> (own ScriptedWriter[E], WriteLog)

impl WriteLog:
    pub fn bytes(self) -> List[u8]
    pub fn writes(self) -> List[List[u8]]
    pub fn finished(self) -> bool
    pub fn dropped(self) -> bool
```

- A `ScriptedReader` returns one chunk per read, cut to `max` with the
  rest kept, then the end, as `ScriptedStream` does today; `failing`
  fails after the last chunk. Split chunks test a line across reads.
- A `ScriptedWriter` records each write, and `failing(after, error)`
  fails the write that passes `after` bytes. The plain `WriteLog`
  outlives the writer, so a test checks `finished()` after the code
  under test called `finish!`.
- `ScriptedNet.add_connection(address, reads)` returns the `WriteLog` of
  the stream it hands out. `ScriptedHttp.reply` serves `send_streaming!`
  with the body as one chunk; `reply_chunks` scripts boundaries.
  `ScriptedProcess.reply` serves `spawn!`, one chunk per pipe. `MemoryFs`
  serves `open!` and `create!`, writing through on each `write!`.

```text
use std.io.{LineReader, ScriptedReader}

fn count_errors![R < own Reader](source: ^mut R) -> Result[usize, LineError[R::Error]]:
    let own lines = LineReader::new(source)
    let count: usize = 0
    while true:
        line := match lines.read_line!()?:
            .Some(text) => text
            .None => break
        if line.contains("ERROR"):
            count = count + 1
    .Ok(count)

tests:
    use std.testing.assert_equal

    it("counts a line split across reads"):
        let own source: ScriptedReader[NetError] = ScriptedReader::new(["ok\nERR".to_utf8(), "OR disk\n".to_utf8()])
        assert_equal(count_errors!(source), .Ok(1), reason="one error line")
```

The log needs care. The freshness rule stops an owned handle from
holding a plain object that the test also holds, since `own` must stay
the only path to the whole value. So a scripted handle holds an integer
id into a table that std keeps for the program instance, as a real
handle holds a descriptor into the host's table, and the `WriteLog`
reads the same entry. Nothing plain is shared through the owned value.

## 10. Cost Model

- **One buffer.** A loop that clears and reuses its list allocates
  once. The host writes into the list's storage: one copy from the
  operating system into Wasm memory, as Go and Rust copy once into the
  caller's slice. `read!(max)` allocates per call, for code that keeps
  each chunk.
- **Writes copy nothing in hd.** `write!` borrows the list and the host
  reads its storage. A host overrides `write_text!` to write the
  string's own UTF-8 bytes, so it copies nothing either.
- **Lines.** One buffer, grown to the longest line up to `limit`, and
  one string copy per line, which an immutable string needs. One host
  call fills 8 KiB. The newline scan can use the byte scans of
  [SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md).
- **Copy.** One 64 KiB buffer. When both ends are host handles, a host
  may splice without passing bytes through Wasm, as Rust's `io::copy`
  uses `copy_file_range`, `sendfile` or `splice` on Linux. The contract
  of `copy!` allows it, so it is a later optimization.
- **Buffered terminals and drop.** `send!`, `run!` and `read_text!` stay
  one host call each. A drop is one synchronous host call.

No cost here is fixed by a signature: `read_into!` sits beside `read!`,
and a host splice can replace the copy loop.

## 11. Prior Art

| System | Model | End of data | Buffer | Backpressure | Cleanup | Lines |
| --- | --- | --- | --- | --- | --- | --- |
| Rust `std::io` | pull, `Read`/`Write` | `Ok(0)` | caller's `&mut [u8]` | blocking | drop; `BufWriter` flushes on drop, errors lost | `BufRead::lines`, no limit |
| tokio `io` | pull, `AsyncRead`/`AsyncWrite` | `Ok(0)` | caller's `ReadBuf`; `read_buf` appends | `Pending` | drop cancels; `read` is cancel safe | `Lines::next_line`, driven by `while let` |
| Go `io` | pull, `Reader`/`Writer` | `io.EOF` | caller's `[]byte` | blocking | explicit `Close`, usually `defer` | `bufio.Scanner`, 64 KiB limit |
| Node streams | push, with a paused mode | `'end'` event | stream's chunks | `highWaterMark`, `write()` false, `'drain'` | `destroy()`; `pipeline()` | `readline` module |
| WHATWG streams | pull through a reader, with a queue | `{done: true}` | stream's chunks, or BYOB | queuing strategy, `desiredSize` | `cancel(reason)` | `TextDecoderStream` plus a transform |
| Deno 2 | WHATWG streams | as WHATWG | as WHATWG | as WHATWG | `using`, `close()` | `TextLineStream` in `@std/streams` |
| Effect `Stream` | pull, chunked, `Stream<A, E, R>` | stream ends | `Chunk` | pull | `Scope`; finalizers run on interruption | `Stream.splitLines` |

**What hd takes.** Rust's and Go's pull reader into a caller's
buffer, with zero bytes at the end, and their plain copy loop; their
blocking is hd's suspension. tokio's append semantics (`read_buf`), its
cancel-safe reads, and its loop-driven line producer. Go's line limit
and its writer that writes every byte or fails. embedded-io's
associated `Error` type, so each module keeps its own enum. Rust's drop
that closes, with a consuming call for what drop cannot report. Deno's
buffered and streaming forms side by side: `Command.output()` beside
`spawn()`, `response.text()` beside `response.body`.

**What hd leaves.** Push streams and queues (Node, WHATWG) add a
scheduler concept and settings that a pull model does not need. Deno 2
dropped `Deno.Reader`, `Deno.Writer`, `Deno.copy` and `Deno.iter` for
WHATWG streams to match the browser; hd has no browser API to match.
Effect's stream combinators (`map`, `filter`, `merge`) can be a library
over these traits later. Go's single `error` from `io.Copy` hides which
side failed, which `CopyError` says.

## 12. Recommendation

Adopt section 2 in step 4 of the ownership migration ("handles with
streaming"): `std.io` with its two traits, `LineReader`, `copy!`, two
errors, two fakes and their log; the new terminals of the table; the
socket handles rewritten as owned, without `close`, `ResourceError` or
`Closed`; and `timeout!` taking an `own fn!()`. `Http.send_streaming!`
gets a default body over `send!`, which copies the body into an owned
reader, so providers that do not stream keep working. `Process.spawn!`,
`FsRead.open!` and `FsWrite.create!` have no honest default and are host
changes.

Not proposed: seeking and random access, `OpenOptions`, an append
writer, a consuming `for` over a suspending producer, stream
combinators, an hd-side buffered writer (the host buffers files), UDP
in this shape (a datagram is not a stream), and stdin and stdout as a
`Reader` and `Writer`, which can follow in the same shape when a binary
filter needs them.

## 13. Questions For The Owner

1. **Host trait additions.** Streaming needs four host methods:
   `Http.send_streaming!` (with a default body over `send!`),
   `Process.spawn!`, `FsRead.open!` and `FsWrite.create!`. Each changes
   the host ABI. Recommendation: approve all four.
2. **How a scripted handle reports.** The freshness rule stops an owned
   scripted stream from sharing a plain log with the test. Option A: the
   handle holds an id into a std table, as a real handle holds a
   descriptor, and the test reads a plain `WriteLog`. Option B: no log;
   a test lends the fake and checks it afterwards, so a writer that the
   code under test finishes or drops cannot be checked. Recommendation:
   A.
3. **Invalid UTF-8 in a line.** `read_line!` can fail with
   `LineError.InvalidUtf8(bytes)`, as Rust's `lines()` fails and as
   `read_text!` reports `FsError.InvalidUtf8`. Or it can replace each bad
   sequence with U+FFFD, as `Response.text()` does. Recommendation: fail,
   keeping the bytes, since a silent replacement changes data.
4. **The default line limit.** Go's `Scanner` stops at 64 KiB, which
   breaks on long JSON lines; Rust's `lines()` has no limit, so a peer
   can exhaust memory. Recommendation: 1 MiB, changed with `limit(n)`.
5. **A failing status on a streamed response.** `send_streaming!` can
   read an error body whole and return `HttpError.Status(response)`, as
   `send!` does, so one `match` serves both terminals. Or it can return
   the streamed response for any status and leave the check to the
   caller. Recommendation: the same error as `send!`, reading at most
   1 MiB of the error body so a hostile server cannot exhaust memory,
   with `ok_status` to stream an expected status such as a 404 page.
6. **Streaming uploads.** A request body from a reader needs an owned
   writer whose consuming call ends the body and returns the response;
   putting a reader inside `Request` would make the plain request owned.
   Recommendation: not now; uploads stay buffered until a program needs
   more.

## Sources

Read on 2026-10-17.

- Rust: [`std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html), [`BufRead::lines`](https://doc.rust-lang.org/std/io/trait.BufRead.html#method.lines), [`io::copy`](https://doc.rust-lang.org/std/io/fn.copy.html), [`BufWriter`](https://doc.rust-lang.org/std/io/struct.BufWriter.html), [`BufReader::get_mut`](https://doc.rust-lang.org/std/io/struct.BufReader.html#method.get_mut)
- tokio: [`AsyncReadExt::read_buf`](https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html#method.read_buf), [`Lines::next_line`](https://docs.rs/tokio/latest/tokio/io/struct.Lines.html), [cancel safety in `select!`](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety), [`Command::kill_on_drop`](https://docs.rs/tokio/latest/tokio/process/struct.Command.html#method.kill_on_drop)
- embedded-io: [`ErrorType`](https://docs.rs/embedded-io/latest/embedded_io/trait.ErrorType.html)
- Go: [`io.Reader`](https://pkg.go.dev/io#Reader), [`io.Copy`](https://pkg.go.dev/io#Copy), [`bufio.Scanner`](https://pkg.go.dev/bufio#Scanner), [`exec.Cmd.StdoutPipe`](https://pkg.go.dev/os/exec#Cmd.StdoutPipe), [`TCPConn.CloseWrite`](https://pkg.go.dev/net#TCPConn.CloseWrite)
- Node: [Stream](https://nodejs.org/api/stream.html), [`stream.pipeline`](https://nodejs.org/api/stream.html#streampipelinesource-transforms-destination-callback)
- WHATWG: [Streams Standard](https://streams.spec.whatwg.org/)
- Deno: [Deno 1.x to 2.x migration guide](https://docs.deno.com/runtime/reference/migration_guide/), [`Deno.Command`](https://docs.deno.com/api/deno/~/Deno.Command)
- Effect: [Stream](https://effect.website/docs/stream/introduction/)

## Parse Log

Every `text` block, 14 in all, was parsed on 2026-10-17 with the existing
`target/debug/examples/parse_check` binary, as written, ownership marks
included; nothing was built. All 14 parse. Parsing checks syntax only:
nothing was type-checked, and the blocks call functions this study does
not define. Two spellings were changed to parse: an associated type
takes no bound in today's grammar, so `type Error` carries its `Error`
bound as a comment, and the top-level `connect!` signature has the
spec's `panic("stub")` body.
