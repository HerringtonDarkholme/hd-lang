# Streaming: One Reader And Writer Shape

Today an hd program reads an HTTP body, a child process's output or a
file in one piece: `send!`, `run!` and `read_text!` return the whole
thing. That is right for most programs and fails for the rest: a
download larger than memory, a build whose output should appear as it
runs, a log of several gigabytes, a line protocol over a socket. This
design adds streaming beside the buffered calls. HTTP bodies, child
pipes, files and TCP streams all implement one `Reader` and one `Writer`
shape, so the same line reader, copy loop and timeout work on any of
them.

A handle is an ordinary value. A `using` binding closes it when its
scope ends ([Using Bindings](../../spec/lang/06-control-flow.md#using-bindings)),
and an explicit `finish!()` closes it and reports any close error. A
call on a closed handle returns the module's `Closed` error, and the
host reports handles a program forgot to close. On the host boundary a
handle is a plain descriptor id that std wraps, as Rust's `File` wraps
a file descriptor.

This is the design for issue #350; nothing in it is implemented yet,
and every name is a placeholder. It adds one
reader shape beside the buffered terminals, as
[the standard library convention](STDLIB.md) allows.

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
terminal beside its buffered one. The terminal returns a plain handle,
which implements `Disposable` and `Reader`, `Writer` or both, and
reports the source module's own error.

```text
pub trait ErrorType:
    type Error                                   # implements std.error.Error

pub trait Reader < ErrorType:
    fn read_into!(&mut self, buffer: &mut List[u8], max: usize) -> Result[usize, Self::Error]
    fn read!(&mut self, max: usize) -> Result[List[u8], Self::Error]     # default: a new list, then read_into!
    fn read_all!(&mut self) -> Result[List[u8], Self::Error]            # default: read_into! until the end

pub trait Writer < ErrorType:
    fn write!(&mut self, bytes: List[u8]) -> Result[void, Self::Error]
    fn write_text!(&mut self, text: string) -> Result[void, Self::Error]  # default: write! of the UTF-8 bytes
    fn flush!(&mut self) -> Result[void, Self::Error]                    # default: .Ok(())
    fn finish!(&mut self) -> Result[void, Self::Error]                   # default: flush!; a handle also closes
```

| Source | Buffered terminal (unchanged) | Streaming terminal (new) | Handle | Error |
| --- | --- | --- | --- | --- |
| HTTP | `Request.send!`, `Client.send!` | `send_streaming!` | `StreamedResponse`: `Reader` | `HttpError` |
| Process | `Command.run!` | `Command.spawn!` | `Child` with `PipeReader?` and `PipeWriter?` fields | `ProcessError` |
| Files | `read_text!`, `write_text!` | `open!`, `create!` | `FileReader`, `FileWriter` | `FsError` |
| TCP | none | `connect!`, `accept!` | `&dyn TcpStream`: both | `NetError` |

1. **Pull, not push.** Nothing is read until the program reads. A read
   returns at least one byte, or zero bytes at the end.
2. **Plain handles, closed by `using`.** Every handle is a plain
   value that implements `Disposable`. A `using` binding disposes it at
   scope end, on every exit and on cancellation. Disposing closes it;
   disposing a closed handle does nothing. There is no `close` method.
3. **Shared use.** Reads and writes take `&mut self`, so a helper takes
   `&mut R` or `&mut dyn Reader[Error = E]`, and the caller holds the same
   handle afterwards.
4. **Module errors, with `Closed`.** A handle reports its module's enum
   through the associated type `Error`. Any call on a closed handle
   returns that enum's `Closed`.
5. **`finish!` reports.** A writer whose close can fail is finished with
   `finish!()`, which flushes, closes and returns the error. A disposed
   writer closes silently.
6. **Leaks are reported.** A handle that is never disposed or finished
   stays in the host's descriptor table. `hd test` fails a test that
   leaves one open, and `hd run` closes leftovers at exit.

`Reader` and `Writer` do not extend `Disposable`: a `LineReader` or an
in-memory writer holds no host resource. Each handle type implements
`Disposable` beside them.

## 3. A Tour

### 3.1 Reading A Body Chunk By Chunk

A downloader shows progress while it hashes a large file:

```text
use std.digest.Sha256
use std.http.{Http, Request}

fn fetch_digest!(url: string) -> Result[string, &dyn Error] $ Http + Console:
    using response = Request::get(url).send_streaming!()?
    total := response.header("content-length").unwrap_or("?")
    let mut hasher: Sha256 = Sha256::new()
    let mut buffer: List[u8] = []
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
`using` binding disposes the response, and a body not read to its end
is aborted, as `fetch` aborts through an `AbortController`. If
`send_streaming!` fails, `?` returns before the binding exists, so there
is nothing to dispose. The buffer keeps its capacity across `clear()`,
so the loop allocates once. A body cut short of its `Content-Length` is
an error, never an early end, so a truncated download cannot pass as
complete.

### 3.2 Lines From A Child Process

A build runner prints each warning while `cargo` runs:

```text
use std.io.LineReader
use std.process.{Command, Process, Stdio}

fn build!(dir: Path) -> Result[usize, &dyn Error] $ Process + Console:
    using child = Command::new("cargo").arg("build").current_dir(dir).stdout(Stdio.Inherit).spawn!()?
    err := child.stderr.expect("stderr is piped by default")
    lines := LineReader::new(err)
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

`spawn!` returns a `Child` whose pipes are plain `Option` fields. The
program reads the pipe straight from the field: `err` and
`child.stderr` are the same handle. Only `child` is bound with `using`,
because disposing a `Child` also disposes the pipes it holds.

`wait!` first closes a stdin pipe still open, so a child that reads to
the end of its input finishes. It reads any pipe still open to its end
into the `ProcessOutput`, closes every pipe, and reaps the process. A
non-zero exit is `.Err(ProcessError.Exited(output))`, as `run!` reports
it. After `wait!`, disposing the child does nothing. Disposing a `Child`
not waited for kills its process group, as cancelling `run!` does, so a
cancelled build or an early `?` leaves no orphan.

```text
pub struct Child:
    pub stdin: PipeWriter?
    pub stdout: PipeReader?
    pub stderr: PipeReader?

impl Disposable for Child:
    fn dispose(&mut self) -> void                  # kills the process group if still running, closes the pipes

impl Child:
    pub fn wait!(&mut self) -> Result[ProcessOutput, ProcessError]

impl Command:
    pub fn spawn!(self) -> Result[&mut Child, ProcessError] $ Process
    pub fn stdout(&mut self, mode: Stdio) -> &mut Command
    pub fn stderr(&mut self, mode: Stdio) -> &mut Command

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

fn download!(url: string, to: Path) -> Result[u64, &dyn Error] $ Http + FsWrite:
    using response = Request::get(url).send_streaming!()?
    using file = create!(to)?
    copied := copy!(response, file)?
    file.finish!()?
    .Ok(copied)
```

`copy!` reads and writes through the two handles the caller holds,
copies through one buffer until the reader ends, and returns the byte
count. It neither finishes nor disposes either handle, as Go's
`io.Copy` closes neither. `finish!()` flushes and closes the file, and
reports a full disk or a failed close; at scope end the `using`
disposal of `file` then does nothing. If `copy!` fails, both bindings
dispose, `file` first: the file closes silently with what was written,
and the request is aborted. A program that must not leave a partial
file writes to a temporary path and calls `rename!` after `finish!`.

```text
pub enum CopyError[R, W]:
    Read(error: R)
    Write(error: W)

pub fn copy![R < Reader, W < Writer](from: &mut R, to: &mut W) -> Result[u64, CopyError[R::Error, W::Error]]:
    let mut buffer: List[u8] = []
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

fn lookup!(key: string) -> Result[string?, &dyn Error] $ Net:
    using stream = connect!("cache.local", 6379)?
    lines := LineReader::new(stream)
    stream.write_text!("GET $key\n")?
    match lines.read_line!()?:
        .Some(reply) => .Ok(reply.strip_prefix("VALUE "))
        .None => .Ok(.None)
```

The program writes to `stream` directly while `lines` reads from the
same handle. Writing is safe, because the line reader buffers only
bytes read. When `lookup!` returns, the `using` binding disposes
`stream`, which closes the socket; `lines` holds no resource and needs
no binding.

`TcpStream` becomes:

```text
pub trait TcpStream < Reader[Error = NetError] & Writer[Error = NetError] & Disposable:
    fn set_timeout(&mut self, timeout: Duration?) -> Result[void, NetError]
    fn set_nodelay(&mut self, enabled: bool) -> Result[void, NetError]
    fn set_keepalive(&mut self, idle: Duration?) -> Result[void, NetError]
    fn close_write!(&mut self) -> Result[void, NetError]

pub trait TcpListener < Disposable:
    fn accept!(&mut self) -> Result[&mut dyn TcpStream, NetError]

pub fn connect!(host: string, port: u16) -> Result[&mut dyn TcpStream, NetError] $ Net: panic("stub")
```

`read!` and `write!` move to `Reader` and `Writer`. `close` goes:
`dispose` closes silently, and `finish!` closes and reports.
`close_write!` tells the peer the request is complete while the program
still reads the reply, as Go's `TCPConn.CloseWrite` does. `finish!` on a
stream is `close_write!` followed by the close. The `ResourceError`
wrapper goes from every signature, and `NetError` gains `Closed`.

### 3.5 Writing To A Child

A program pipes generated text into `sort` and reads the result:

```text
use std.task.all

fn sorted!(lines: List[string]) -> Result[string, &dyn Error] $ Process:
    using child = Command::new("sort").spawn!()?
    input := child.stdin.expect("stdin is piped by default")
    output := child.stdout.expect("stdout is piped by default")
    let (written, text) = all!(feed(input, lines), output.read_all())
    written?
    child.wait!()?
    .Ok(string::from_utf8(text?)?)

fn feed!(input: &mut PipeWriter, lines: List[string]) -> Result[void, ProcessError]:
    for line in lines:
        input.write_text!("$line\n")?
    input.finish!()
```

`feed!` finishes the writer, which closes the child's stdin, so `sort`
sees the end of its input. The two children of `all!` run together, so
neither pipe can fill while the other waits, and `all!` returns both
results. If the caller is cancelled, the `using` binding disposes
`child`: its process group is killed and both pipes close.

## 4. Line And Chunk Iteration

Chunk iteration needs no type: a `while` loop over `read_into!`, as in
3.1. Lines need a buffer that outlives each call, so they get a named
producer:

```text
pub struct LineReader[R]:
    source: &mut R
    buffer: List[u8]                          # bytes read but not yet returned
    limit: usize = 1048576

impl[R < Reader] LineReader[R]:
    pub fn new(source: &mut R) -> &mut LineReader[R]
    pub fn limit(&mut self, max: usize) -> &mut LineReader[R]
    pub fn read_line!(&mut self) -> Result[string?, LineError[R::Error]]
    pub fn read_until!(&mut self, byte: u8) -> Result[List[u8]?, LineError[R::Error]]

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

**One handle, two holders.** `LineReader::new(source)` keeps the handle
the program passed, not a copy, so the reader and the program hold the
same handle. The cost is nothing: no copy, no extra host call, and no
second descriptor. The hazard is a direct read. Once the reader has
refilled, up to 8 KiB of the stream sit in its buffer, so a read on
the handle itself returns the bytes after them, and the data arrives
out of order with no error. Rust's `BufReader::get_mut` documents the
same hazard. So a program that wraps a source reads only through the
reader, which serves bytes as well as lines. Writing to the handle, as
in 3.4, and setting options are safe. A reader whose source was closed
first returns the complete lines it holds, then
`.Err(LineError.Read(Closed))`. A `LineReader` holds no host resource,
so it is not `Disposable`, and closing it means closing its source.

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

## 6. Cancellation And Disposal

- **A frame is cancelled.** Its `using` handles dispose, in reverse
  order of declaration, with the frame's other synchronous cleanup. A
  socket closes, a file closes, an HTTP request is aborted, and a
  child's process group is killed. A writer is not finished, so nothing
  more is flushed over the network.
- **A read is cancelled.** It returns nothing, and the bytes the host
  had not returned stay for the next read on that handle, as the net
  spec's rule for `read!` already says, extended to every `Reader`.
  tokio documents the same cancel safety for `read`.
- **A line read is cancelled.** The partial line stays in the
  `LineReader`'s buffer, so the next `read_line!` returns the whole
  line.
- **A write is cancelled.** An unknown number of bytes went out. The
  handle stays usable, but its position in the protocol is unknown, so
  a framed protocol closes it.
- **A copy is cancelled.** Bytes read but not yet written are lost, as
  with tokio's `copy`.
- **A handle is closed during an operation.** One task can finish or
  dispose a handle while another task's read on it waits. The waiting
  operation ends with `.Err(Closed)`, as the net spec already says for
  a pending operation on a closed stream, extended to every handle.
- **A handle is used after it closes.** Every call returns `Closed`.
  This covers a handle that escaped its `using` scope in a field, a
  closure or a return value: it is still a value, but a closed one. A
  function that returns an open handle binds it with `:=`, and its
  caller binds the result with `using`.
- **A handle is never closed.** The program forgot `using` or
  `finish!`. The host's descriptor table still lists it, so `hd test`
  fails the test that leaked it and `hd run` closes it at exit. There is
  no garbage-collector finalizer, so an unreachable handle stays open
  until then.

A file writer's host may keep a buffer. Its `dispose` asks the host to
close synchronously, which flushes and ignores errors, as Rust's
`BufWriter` and `File` do on drop. `finish!` is the path that reports
them. A network writer keeps no buffer in std, so a disposed one loses
nothing that `write!` acknowledged.

Every std handle implements `Disposable`, whose one method `dispose!`
may suspend, so `using` is written only in a `!` function. A std
handle's dispose is one host call that closes it; a graceful close that
waits for the peer, such as a stream's `close_write!`, is part of
`finish!`. Cancellation does not cut a dispose short: a cancelled task
finishes its disposes, in reverse order, before the cancellation
completes, as JavaScript's `await using` and C#'s `DisposeAsync` do.

Several tasks may use one handle at once, as `map_all!` items or `all!`
children can. A handle serves one read and one write at a time; a later
call waits its turn in call order, as Go's `net.Conn` does. Each
`write!` lands whole, and a socket can still read and write at the
same time.

## 7. Timeouts

```text
use std.task.timeout

fn first_line!(host: string) -> Result[string?, &dyn Error] $ Net + Clock:
    using stream = connect!(host, 25)?
    stream.set_timeout(.Some(5s))?                # each later read waits at most 5s
    lines := LineReader::new(stream)
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

The block is an ordinary closure that captures `lines`, and
`timeout!`'s `work` stays `fn!() -> T $ R`. After the block, the
program still holds `lines` and `stream`. If the timer wins, the read
is cancelled with its bytes kept, and the stream stays open until the
`using` binding disposes it. A `retry!` attempt and the children of
`race!` and `all!` capture handles the same way. Several `map_all!`
items may use one shared handle at once; section 13 asks how such
calls are ordered.

## 8. Errors

Each handle reports its module's enum, as the shared convention
requires: `HttpError`, `ProcessError`, `FsError` or `NetError`. The
associated type `Error` carries it, as the embedded-io crate's
`ErrorType` does, so a generic helper writes `R::Error`. `std.io` names
errors for its own operations only, as the convention allows a pure
module (`DecodeError`, `TimedOut`): `LineError[E]` and `CopyError[R, W]`.
A program that mixes sources returns `&dyn Error`, as the tour does, or
its own enum with `From` impls, which `?` finds.

Each of the four enums gains a `Closed` variant, which replaces
`std.resource.ResourceError` and its `Disposed`. A call on a handle
that was disposed, finished, or closed by another task returns it. A
generic helper sees it as an ordinary `R::Error`, so `LineReader`
reports it as `LineError.Read(Closed)`. `NetError.Timeout` stays for
`set_timeout`.

## 9. Test Fakes

`std.io` adds two fakes, and each scripted provider serves its new
terminal:

```text
impl ScriptedReader:
    pub fn new(chunks: List[List[u8]]) -> &mut ScriptedReader[E]
    pub fn failing(chunks: List[List[u8]], error: E) -> &mut ScriptedReader[E]

impl ScriptedWriter:
    pub fn new() -> (&mut ScriptedWriter[E], WriteLog)
    pub fn failing(after: usize, error: E) -> (&mut ScriptedWriter[E], WriteLog)

impl WriteLog:
    pub fn bytes(self) -> List[u8]
    pub fn writes(self) -> List[List[u8]]
    pub fn finished(self) -> bool
    pub fn disposed(self) -> bool
```

- A `ScriptedReader` returns one chunk per read, cut to `max` with the
  rest kept, then the end, as `ScriptedStream` does today; `failing`
  fails after the last chunk. Split chunks test a line across reads.
- A `ScriptedWriter` records each write, and `failing(after, error)`
  fails the write that passes `after` bytes. The test checks
  `finished()` after the code under test called `finish!`.
- `ScriptedNet.add_connection(address, reads)` returns the `WriteLog` of
  the stream it hands out. `ScriptedHttp.reply` serves `send_streaming!`
  with the body as one chunk; `reply_chunks` scripts boundaries.
  `ScriptedProcess.reply` serves `spawn!`, one chunk per pipe. `MemoryFs`
  serves `open!` and `create!`, writing through on each `write!`.

A scripted handle holds an integer id into a table that std keeps for
the program instance, as a real handle holds a descriptor into the
host's table, and its `WriteLog` reads the same entry. The table gives
fakes the behavior of real handles: `Closed` after disposal, the
ordering of shared calls, and the leak report, since `hd test` checks
std's table beside the host's. The plain `WriteLog` serves streams the
test never holds, such as the one `ScriptedNet` hands to the code under
test.

```text
use std.io.{LineReader, ScriptedReader}

fn count_errors![R < Reader](source: &mut R) -> Result[usize, LineError[R::Error]]:
    lines := LineReader::new(source)
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
        using source: &mut ScriptedReader[NetError] = ScriptedReader::new(["ok\nERR".to_utf8(), "OR disk\n".to_utf8()])
        assert_equal(count_errors!(source), .Ok(1), reason="one error line")
```

The test binds the fake with `using`, as a program binds a real handle.
Without it, the leak report fails the test.

## 10. Cost Model

- **One buffer.** A loop that clears and reuses its list allocates
  once. The host writes into the list's storage: one copy from the
  operating system into Wasm memory, as Go and Rust copy once into the
  caller's slice. `read!(max)` allocates per call, for code that keeps
  each chunk.
- **Writes copy nothing in hd.** `write!` takes the list by reference
  and the host reads its storage. A host overrides `write_text!` to
  write the string's own UTF-8 bytes, so it copies nothing either.
- **Lines.** One buffer, grown to the longest line up to `limit`, and
  one string copy per line, which an immutable string needs. One host
  call fills 8 KiB. The newline scan can use the byte scans of
  [SIMD Byte-Scan Primitives](SIMD_PRIMITIVES.md). Sharing the source
  handle costs nothing.
- **Copy.** One 64 KiB buffer. When both ends are host handles, a host
  may splice without passing bytes through Wasm, as Rust's `io::copy`
  uses `copy_file_range`, `sendfile` or `splice` on Linux. The contract
  of `copy!` allows it, so it is a later optimization.
- **Buffered terminals and disposal.** `send!`, `run!` and `read_text!`
  stay one host call each. A `dispose` is one synchronous host call;
  disposing a closed handle is a flag test in hd. The `Closed` check
  rides on the table lookup every host call already does.
- **Leak report.** One walk of the descriptor table, and of std's
  scripted table, at the end of each test and at exit.

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
cancel-safe reads, and its loop-driven line producer. Go's line limit,
its writer that writes every byte or fails, and its `Conn`, which
several goroutines may call at once. embedded-io's associated `Error`
type, so each module keeps its own enum. Deno's and JavaScript's
`using` with a dispose method for scoped cleanup, and Go's explicit
`Close` for the close that reports, here `finish!`. Deno's buffered and
streaming forms side by side: `Command.output()` beside `spawn()`,
`response.text()` beside `response.body`.

**What hd leaves.** Push streams and queues (Node, WHATWG) add a
scheduler concept and settings that a pull model does not need. Deno 2
dropped `Deno.Reader`, `Deno.Writer`, `Deno.copy` and `Deno.iter` for
WHATWG streams to match the browser; hd has no browser API to match.
Effect's stream combinators (`map`, `filter`, `merge`) can be a library
over these traits later. Go's single `error` from `io.Copy` hides which
side failed, which `CopyError` says. Rust's compile-time exclusive
borrow of a wrapped reader is out of reach with plain handles, so the
direct-read hazard of section 4 is documented, not checked.

## 12. Recommendation

Adopt section 2 once `Disposable` and `using` land: `std.io` with its
two traits, `LineReader`, `copy!`, two errors, two fakes and their log;
the new terminals of the table; the socket handles rewritten as
`Disposable`, without `close` or `ResourceError` and with
`NetError.Closed`; `Closed` in the other three module errors; and the
host's leak report covering every handle. `timeout!`, `race!`, `all!`,
`retry!` and `map_all!` keep their closure parameters unchanged.
`Http.send_streaming!` gets a default body over `send!`, which copies
the body into an in-memory reader, so providers that do not stream keep
working. `Process.spawn!`, `FsRead.open!` and `FsWrite.create!` have no
honest default and are host changes.

Not proposed: seeking and random access, `OpenOptions`, an append
writer, a consuming `for` over a suspending producer, stream
combinators, an hd-side buffered writer (the host buffers files), UDP
in this shape (a datagram is not a stream), and stdin and stdout as a
`Reader` and `Writer`, which can follow in the same shape when a binary
filter needs them.

## 13. The Main Choices In Brief

- Four host methods carry streaming: `Http.send_streaming!` (with a
  default body over `send!`), `Process.spawn!`, `FsRead.open!` and
  `FsWrite.create!`.
- A scripted handle holds an id into a std table, as a real handle holds
  a descriptor, and the test reads a plain `WriteLog` (section 9).
- `read_line!` fails on invalid UTF-8 with `LineError.InvalidUtf8(bytes)`,
  keeping the bytes, since a silent replacement changes data.
- The default line limit is 1 MiB, changed with `limit(n)`.
- A failing status on a streamed response is the same error as `send!`
  returns, reading at most 1 MiB of the error body so a hostile server
  cannot exhaust memory; `ok_status` streams an expected status such as
  a 404 page.
- Uploads stay buffered for now. A request body from a reader needs a
  writer whose `finish!` ends the body and returns the response, a new
  terminal shape that waits for a program that needs it.

## Sources

Read on 2026-10-17.

- Rust: [`std::io::Read`](https://doc.rust-lang.org/std/io/trait.Read.html), [`BufRead::lines`](https://doc.rust-lang.org/std/io/trait.BufRead.html#method.lines), [`io::copy`](https://doc.rust-lang.org/std/io/fn.copy.html), [`BufWriter`](https://doc.rust-lang.org/std/io/struct.BufWriter.html), [`BufReader::get_mut`](https://doc.rust-lang.org/std/io/struct.BufReader.html#method.get_mut)
- tokio: [`AsyncReadExt::read_buf`](https://docs.rs/tokio/latest/tokio/io/trait.AsyncReadExt.html#method.read_buf), [`Lines::next_line`](https://docs.rs/tokio/latest/tokio/io/struct.Lines.html), [cancel safety in `select!`](https://docs.rs/tokio/latest/tokio/macro.select.html#cancellation-safety), [`Command::kill_on_drop`](https://docs.rs/tokio/latest/tokio/process/struct.Command.html#method.kill_on_drop)
- embedded-io: [`ErrorType`](https://docs.rs/embedded-io/latest/embedded_io/trait.ErrorType.html)
- Go: [`io.Reader`](https://pkg.go.dev/io#Reader), [`io.Copy`](https://pkg.go.dev/io#Copy), [`bufio.Scanner`](https://pkg.go.dev/bufio#Scanner), [`exec.Cmd.StdoutPipe`](https://pkg.go.dev/os/exec#Cmd.StdoutPipe), [`TCPConn.CloseWrite`](https://pkg.go.dev/net#TCPConn.CloseWrite), [`net.Conn`](https://pkg.go.dev/net#Conn)
- Node: [Stream](https://nodejs.org/api/stream.html), [`stream.pipeline`](https://nodejs.org/api/stream.html#streampipelinesource-transforms-destination-callback)
- WHATWG: [Streams Standard](https://streams.spec.whatwg.org/)
- Deno: [Deno 1.x to 2.x migration guide](https://docs.deno.com/runtime/reference/migration_guide/), [`Deno.Command`](https://docs.deno.com/api/deno/~/Deno.Command)
- JavaScript: [Explicit Resource Management proposal](https://github.com/tc39/proposal-explicit-resource-management)
- Effect: [Stream](https://effect.website/docs/stream/introduction/)

## Parse Log

Every `text` block, 14 in all, was parsed on 2026-10-17 with the existing
`target/debug/examples/parse_check` binary; nothing was built. Seven
parse as written. The other seven fail only at `using`, which today's
grammar does not have yet; with each `using` replaced by `let`, all 14
parse. Parsing checks syntax only: nothing was type-checked, and the
blocks call functions this study does not define. Two spellings were
changed to parse: an associated type takes no bound in today's grammar,
so `type Error` carries its `Error` bound as a comment, and the
top-level `connect!` signature has the spec's `panic("stub")` body.
