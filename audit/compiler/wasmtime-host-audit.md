# Wasmtime Host Audit (#160, step 1)

Status: audit 2026-10-13. The owner approved S1-S12 (now in `spec/std`)
and answered D1 with "use an async runtime". F0 is done. On 2026-10-14
the fix plan was reworked around tokio (§4 design, §5 plan); no code
changed in that rework.

Scope: `crates/hd_run_wasmtime` at `d2e81fa5` (10.4k production lines,
7.5k test lines). Node's `host/*.mjs` is a reference, not a norm.

## Summary

- Only `Console`, `ConsoleInput`, `Clock.sleep` and `hd:rt` waits
  suspend. Every other bang host call (`FsRead`, `FsWrite`,
  `Process.run!`, `Http.send!`, every `Net` method) does its whole job
  inside `.start` and returns status 0
  ([`host/call.rs:151-224`](../../crates/hd_run_wasmtime/src/host/call.rs),
  [`Ret::ready`, `host/reply.rs:139`](../../crates/hd_run_wasmtime/src/host/reply.rs)).
  Their `.finish` imports are `Kind::NeverFinish` (`host/kind.rs:222,249,251`).
- So `all!` and `race!` over these calls run one after another. A
  `race!` timeout around them never fires. A REPL Ctrl-C waits until the
  call returns. A `Net` read, accept or receive with no `--time-limit`
  can block forever.
- This was a recorded choice: engines-and-test-runner.md §20.3 and §20.6
  Q1 ("every `.start` still completes synchronously"). It reverses
  runtime-and-host.md §17.5, which planned a reactor with real abort.
  The ABI already allows pending results for all of these methods
  (`hd_host_abi` marks them `Wait::May`), so the fix is host-only.
- Console output was held in memory until exit, a stderr write or a
  `read_line!`, so a program that printed and then waited showed
  nothing. Fixed by F0: `Host::wait` flushes first, and so does output
  past 64 KiB.
- 126 external crates on macOS (147 over all targets). wasmtime brings
  74 of them. `url` alone brings 26 crates that nothing else uses
  (IDNA and ICU4X tables).

## Probes

Run with a cold `--features wasmtime` dev build in
`target/audit-160/tgt` (sources in `target/audit-160/*.hd`).

| Probe | Expected if suspending | wasmtime | Node |
| --- | --- | --- | --- |
| `println("before")`, `sleep!(1.5s)`, `println("after")` | `before` shows 1.5 s before `after` | both lines at exit | both lines at exit |
| `all!(run!("sleep", ["1"]), sleep!(1s))` | ~1 s | 2.10 s; with the order swapped, 1.07 s | not run |
| `race!(listener.accept!(), sleep!(500ms))`, `--time-limit 4s` | `timeout` at 0.5 s | `time-limit` panic at 4.09 s | hung past 2 min; killed (ignores `--time-limit`) |

## 1. Blocking Versus Suspending

"Main" is the program thread that runs Wasm. "Deadline" is
`--time-limit` or a test `timeout`. "Stop" is the REPL Ctrl-C
(`Stopper`, `session.rs:89-106`; S2 stop, S3 cancel).

| Capability | Mode | Where the work runs | Other tasks while it waits | Deadline | Stop / cancel | Verdict |
| --- | --- | --- | --- | --- | --- | --- |
| `Console.write_line!` | completes at once in the reactor (`reactor.rs:128`) | main; bytes go to `Host::out` | not applicable | n/a | n/a | OK: held output is flushed before every host wait (`Host::wait`) and once it passes 64 KiB (`HELD_OUTPUT_BOUND`, `host/state.rs`); F0 done |
| `ConsoleInput.read_line!` | suspends: handle, line delivered by a detached reader thread (`reactor.rs:176-211`) | reader thread | run | wait cut (`reactor.rs:286-338`) | wakes the wait; cancel runs `defer` | OK |
| `Clock.now`, `monotonic` | plain | main | n/a | n/a | n/a | OK |
| `Clock.sleep!` | suspends: reactor timer (`reactor.rs:133`) | main, parked on a condvar | run | wait cut | wakes; cancel drops the timer | OK |
| `hd:rt` `block` (`block_on`) | loops on the reactor (`host/call.rs:258-266`) | main | run | cut | wakes | OK |
| `Random`, `Sys`, `Env`, `Args` | plain, fast syscalls | main | n/a | n/a | n/a | OK |
| `FsRead`, `FsWrite` | blocks inside `.start` (`host/call.rs:151-175`) | main | blocked | regular files: checked before and after only; FIFOs and Windows pipes: polled every 50 ms (`fs/io.rs:69-213`); no deadline means `read_to_end` with no bound (`fs/io.rs:83-87`) | flag seen only after the call returns, then `stopped`, not `cancelled` | Partially: fast for small local files; a FIFO, a slow disk or NFS blocks the whole program |
| `Process.run!` | blocks inside `.start` (`host/call.rs:176-196`) | three pipe threads per run plus main polling every 50 ms (`core/workers.rs:70-249`) | blocked (probe: `all!` 2.10 s) | kills the child, not its descendants | ignored until the child exits | Blocks the whole program |
| `Http.send!` | blocks inside `.start` (`host/call.rs:197-202`) | one process-wide worker thread (`http/client.rs:121-133`); concurrent instances (`hd test --jobs`) queue behind each other | blocked | request timeout (default 30 s, `http/request.rs:45`) or deadline; with neither, `recv()` with no bound (`http/client.rs:68-72`) | ignored; `race!` cannot cancel (`std-http.send.cancel` is never reachable) | Blocks the whole program |
| `Net` (all methods) | blocks inside `.start` (`host/call.rs:203-224`) | one worker per instance, busy-polling nonblocking sockets every 5 ms (`net/state.rs:56-77`, `net/mod.rs:30`) | blocked (probe: `race!` timeout never fires) | cut; with no deadline, `recv()` with no bound (`net/client.rs:240-243`) | ignored; in the REPL Ctrl-C does nothing, since `ctrlc` replaced SIGINT (`hd_cli/src/repl.rs:164`) and the REPL sets no time limit (`repl.rs:372`) | Blocks the whole program |
| `TestRunner` / `PropertyRunner` | plain, small fs work | main | n/a | checked after `snapshot_check` and `start` | n/a | OK |

Other `Net` findings, independent of blocking:

| Finding | Pointer | Effect |
| --- | --- | --- |
| `TcpStream.close` only shuts down the write half and keeps the socket | `net/ops.rs:30-33` | the descriptor stays open until the instance ends |
| `arrivals` reads every socket, closed ones included, into an unbounded inbox | `net/state.rs:218-252` | a peer that keeps sending grows host memory with no bound, outside `--max-heap`; TCP flow control never pushes back |
| The worker wakes every 5 ms while it exists, even idle | `net/state.rs:59` | 200 wakeups a second for the rest of the run after the first `Net` call |

Other host-buffer findings: an `Http` body is read with no size limit
(`http/request.rs:209-212`, ureq's `with_config()` defaults to
`u64::MAX`). `Process` output is capped at 1 GiB per run
(`core/process.rs:61`). Neither counts against `--max-heap`, which
runtime-and-host.md §17.8 planned ("host-side buffers").

## 2. Dependencies

`cargo tree -p hd_run_wasmtime -e normal --depth 1` (aarch64 macOS):

```text
getrandom 0.4.3, hd_cache, hd_host_abi, hd_run, hd_wasm, hostname 0.4.2,
libc 0.2.190, nix 0.31.3, rustix 1.1.5, rustls 0.23.45, serde_json 1.0.152,
socket2 0.6.5, ureq 3.4.2, url 2.5.8, wasmtime 49.0.2
Windows only: interprocess 2.4.4, winapi-util 0.1.11, windows-sys 0.61
```

Transitive totals: 126 external crates on macOS, 129 on
x86_64-pc-windows-msvc, 147 across all targets (`--target all`).

"Total" counts the crates under that direct dependency. "Only" counts
the crates no other direct dependency pulls, which removing it would
save.

| Dependency | Used for | Total | Only | Std or smaller crate? |
| --- | --- | --- | --- | --- |
| `wasmtime` | the engine | 74 | 57 | no; features are already minimal (`Cargo.toml:25-32`) |
| `url` | `granted_url` parse and redirect `join` (`http/client.rs:161`, `http/request.rs:127`) | 35 | 26 (idna, icu_*, zerovec, yoke, ...) | yes: `http::Uri` is already in the tree via ureq, plus a small RFC 3986 join. Or pin `idna_adapter` to 1.0 (ASCII only) or 1.1 (unicode-rs) to drop ICU4X. Depends on question S9 |
| `ureq` (rustls feature) | HTTP/1.1, TLS, timeouts | 22 (incl. rustls, ring, webpki-roots) | 8 | keep for now. About 900 lines (`http/transport.rs`, `chunk.rs`, most of `headers.rs`) wrap ureq's transport only to keep raw header spelling and cross-name order for Node parity. Question S8 decides whether they can go |
| `rustls` | only `is::<rustls::Error>` to classify TLS errors (`http/request.rs:234`) | 12 | 0 | free (ureq pulls it); a direct dependency only for one type check |
| `nix` | `poll`/`select` for FIFOs and process pipes, `fcntl`, test `mkfifo` | 4 | 1 | `rustix` (already in the tree, 0 new crates) covers poll, fcntl and mkfifo; the FIFO polling may go entirely (jobs T7, T12) |
| `rustix` | one errno downcast of wasmtime's allocation error (`limits.rs:34`) | 4 | 0 | free; pinned to wasmtime's version |
| `libc` | four errno and flag constants (`net/socket.rs:148`, `fs/io.rs:265,279`, `fs/ops.rs:302`) | 1 | 0 | free; `rustix` or `io::ErrorKind` would do |
| `socket2` | nonblocking connect with per-candidate timeouts | 2 | 1 | std `TcpStream::connect_timeout` is enough once each connect runs on its own thread |
| `getrandom` | `Random` | 3 | 1 | keep (std has no stable entropy API); ring also pulls getrandom 0.2, so two versions build |
| `hostname` | `Sys.hostname` | 3 | 1 | `rustix::system::uname().nodename()` on Unix, `windows-sys` on Windows; saves one crate |
| `serde_json` | test-runner JSON lines (`testing_json.rs`, `testing_files.rs`) | 5 | 2 | keep; `hd_cli` links it anyway |
| `interprocess`, `winapi-util`, `windows-sys` | Windows named-pipe paths in `Fs` and pipe handles for `Process` (`windows_pipe.rs`, 386 lines) | Windows only | 3 + deps | removable with the Node-parity pipe handling if question S6 says special files get no deadline guarantee |
| `hd_*` | cache, ABI table, driver, linker | 12 | 1 | in-repo |

Build cost: a cold dev build of `hd_cli --features wasmtime` in a fresh
target directory took 61 s wall on 14 cores under load average 8
(`--timings`, 540 s of summed crate time). Approximate shares of the
summed time: wasmtime and Cranelift 238 s, ureq + rustls + ring 54 s,
url + IDNA + ICU 24 s, `hd_run_wasmtime` itself 9.7 s, nix 3.1 s. ring
also needs a C compiler (`cc` build script, 5.8 s).

This audit first kept async runtimes out. The owner's D1 answer
reverses that: §4 adds tokio and counts the crates it adds and removes.

## 3. Spec

### Divergences

| Rule | wasmtime behavior |
| --- | --- |
| [`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort), [`std-http.send.cancel`](../../spec/std/http.md#r-std-http.send.cancel) | met only because nothing is ever pending; `race!` cannot cancel a request, a child process or a socket wait |
| [`req.waker.arrange`](../../spec/lang/11-requirements-and-suspension.md#r-req.waker.arrange) with [`req.schedule.all-unfinished`](../../spec/lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished) | `all!` children run one after another: the first blocking start holds the rest |
| [`cli.repl.cancel`](../../spec/cli/command-line.md#r-cli.repl.cancel) (Stop while waiting on the host runs `defer`) | a Stop during a blocking host call is seen after the call returns and reports `stopped`, not `cancelled` (`host/call.rs:401`, `instance.rs:269-295`) |
| [`module.process.other`](../../spec/lang/10-modules.md#r-module.process.other) ("cannot start") | output over 1 GiB after the child started becomes `Other` (`core/workers.rs:121-123`, `core/process.rs:72-83`) |

Std rules written from S1-S12 (owner-approved 2026-10-13) that the
wasmtime host does not meet yet. `KNOWN_FAILURES.tsv` cannot list them:
it holds parse-phase fixture paths only, and the corpus test fails on a
row it does not see. Each fix job removes its rows here.

| Rule | wasmtime behavior | Fix job |
| --- | --- | --- |
| [`std-host.op.wait`](../../spec/std/host.md#r-std-host.op.wait), [`std-fs.special-file`](../../spec/std/fs.md#r-std-fs.special-file) | `Fs`, `Process`, `Http` and `Net` calls block the whole instance | T2, T4, T6, T7, T8 |
| [`std-process.cancel`](../../spec/std/process.md#r-std-process.cancel) | cancel is not seen until the child exits; a deadline kills the child, not its process group | T2 (Windows: question Q1) |
| [`std-http.send.cancel.close`](../../spec/std/http.md#r-std-http.send.cancel.close) | a request cannot be aborted, so its connection is never closed early | T6 |
| [`std-net.cancel.read`](../../spec/std/net.md#r-std-net.cancel.read), [`std-net.close.pending`](../../spec/std/net.md#r-std-net.close.pending) | no `Net` operation is ever pending, so neither can be observed | T4 (`cancel.read`), T5 (`close.pending`) |
| [`std-net.close.release`](../../spec/std/net.md#r-std-net.close.release) | `close` shuts down only the write half and keeps the socket (`net/ops.rs:30-33`) | T5 |
| [`std-host.op.heap`](../../spec/std/host.md#r-std-host.op.heap), [`std-host.op.heap.exhausted`](../../spec/std/host.md#r-std-host.op.heap.exhausted), [`std-host.op.heap.no-other-cap`](../../spec/std/host.md#r-std-host.op.heap.no-other-cap) | `Http` bodies, `Process` output and `Net` inboxes are outside `--max-heap`; `Process` output has its own 1 GiB cap | T9 |
| [`std-console.host.visible`](../../spec/std/console.md#r-std-console.host.visible) | met: flushed before every host wait and past 64 KiB (F0 done) | F0 |
| [`std-http.url.non-ascii`](../../spec/std/http.md#r-std-http.url.non-ascii), [`std-http.url.no-normalize`](../../spec/std/http.md#r-std-http.url.no-normalize) | `url` maps a non-ASCII host through IDNA instead of returning `InvalidUrl` | T10 |
| [`std-process.child.signal`](../../spec/std/process.md#r-std-process.child.signal) | a signal-killed child reports 128, without the signal number (`core/pipes.rs:145,149`) | T3 |

### Questions For The Owner (std spec only)

Each proposes a std-spec item. None is applied. Files are `spec/std/`.

| # | Question | Recommendation | Where |
| --- | --- | --- | --- |
| S1 | Must a host operation that waits suspend only its own task, so other `all!`/`race!` children keep running? | Yes. One rule: "a bang host method that waits lets the instance run other tasks until it completes." Without it, `race!` timeouts are not portable | new "Host Operations" section in `host.md` |
| S2 | What does cancelling a pending host operation leave behind? | Process: the child (and its process group) is killed. Http: the connection is closed. Net read/receive: bytes not yet returned stay for the next call. Fs write: may leave a partial file. Say "may have happened" for any write | `host.md`, with one line each in `process.md`, `http.md`, `net.md`, `fs.md` |
| S3 | When does `println` output become visible? | Each completed `write_line!` reaches the host stream before the program next waits on the host or ends. Standard output and error keep their relative order | `console.md` |
| S4 | Should the default `Http` timeout be a number? | Yes, 30 s, as both engines do. A timeout covers the whole request, redirects included | `http.md` (`std-http.request.defaults`) |
| S5 | What does `close` do to a TCP stream? | Releases the socket at once (a full close). A pending operation on it ends with `Disposed` | `net.md` |
| S6 | Do reads of special files (FIFOs, devices, named pipes) have to stop at a deadline? | No guarantee beyond "the program is not blocked". Lets the FIFO and Windows-pipe polling go | `fs.md` |
| S7 | Do host-side buffers (an Http body, Process output, unread socket data) count against `--max-heap`? | Yes: past the budget the operation panics `heap-exhausted`, as runtime-and-host.md §17.8 planned. Drop the separate 1 GiB Process cap | `host.md` (cross-reference `cli.limit.heap`) |
| S8 | Must response headers keep name spelling and the order across different names? | No. Names compare ignoring ASCII case. Only values of one name keep their order (already `std-http.headers.pairs`). This lets the raw-header transport go | `http.md` |
| S9 | Which URLs are valid, and what do grants match? | RFC 3986 absolute `http`/`https` URLs with an ASCII host; a non-ASCII host is `InvalidUrl` (write punycode). Grants match the lowercase ASCII host. No lookalike normalization, and `url` + ICU can go | `http.md` |
| S10 | What status does a child killed by a signal report? | 128 plus the signal number (shell convention). Today it is always 128 (`core/pipes.rs:145,149`) | `process.md` |
| S11 | Do Process children inherit the environment and working directory? | Yes, both, with nothing added. Today the code does this without a spec rule | `process.md` |
| S12 | Is there any order between independent host operations that complete together? | No. Say so, so no engine relies on wake-batch order | `host.md` |

**D1 (answered 2026-10-14): use an async runtime.** Every waiting
`.start` returns a handle, and tokio does the work. This replaces the
sync-only choice of engines-and-test-runner.md §20.6 Q1 and the
thread-per-call plan this audit first proposed. §4 says where tokio sits.

## 4. Async Host Design (tokio)

All paths below are under `crates/hd_run_wasmtime/src`.

### 4.1 Where tokio sits

- **Kept unchanged:** the ABI's start/finish/abort protocol
  (`hd_host_abi` `Wait::May`), Wasm-level suspension, and synchronous
  wasmtime (no `async_support`, as runtime-and-host.md §17.5 already
  decided). No Wasm call waits inside the engine, so no fiber stacks.
- **`.start`** checks the grant and the arguments on the main thread.
  Refusals, unknown handles and `Disposed` stay synchronous, with status
  0, as `Fs` returns today (`host/call.rs:151-175`). Otherwise it
  spawns one tokio task and returns `(1, handle)`.
- **The task** delivers `(handle, Done)` into the instance's reactor
  inbox and notifies its condvar. That inbox already exists: the
  `ConsoleInput` reader thread delivers lines into it
  (`reactor.rs:54-61`, `:187-205`). T1 generalizes it from lines to
  completions.
- **`.finish`** takes the `Done`, encodes it (inserting a new socket into
  the `Net` handle table, for example) and returns the bytes.
  **`hd:rt.abort`** (`Control::Abort`, `host/state.rs:223-228`) aborts
  the task and drops any result already delivered.
- **`Host::wait` stays a condvar wait** (`host/state.rs:285-293`,
  `reactor.rs:286-338`). It is not a `block_on`. This differs from the
  brief's starting point and from §17.5's "`block_on` the reactor" for
  three reasons:
  1. With `block_on` on the program thread, I/O moves only while the
     program waits. A child that fills its 64 KiB pipe during a long
     computation stalls. A dedicated I/O thread keeps pipes and sockets
     moving.
  2. `Clock.sleep`, the `--time-limit` cut, the REPL `Stopper` wake and
     `Waker::parked` (`session.rs:89-106`) and the F0 flush all keep their
     current code. A `block_on` design rewrites each of them on tokio
     timers and `Notify`.
  3. `tokio::process` reaps children only while the runtime is driven
     (`tokio::process` module docs, "Unix Processes"). A runtime driven
     only during waits leaves zombies between waits.

### 4.2 Runtime flavor and lifetime

- **One process-wide current-thread runtime, on a dedicated `hd-io`
  thread** that runs `Runtime::block_on(std::future::pending())`. Tasks
  arrive through `Handle::spawn` from any program thread. Only
  `Runtime::block_on` drives the I/O and timer drivers of a
  current-thread runtime (docs.rs, `tokio::runtime::Handle::block_on`),
  so the dedicated thread is required.
- **Lazy:** a `OnceLock<Handle>`, started by the first offloaded
  operation. This is the pattern of the `hd-epochs` ticker
  (`deadline.rs:9-33`). A program that makes no `Process`, `Http`,
  `Net` or special-file call starts no thread, so `hd run hello.hd` is
  unchanged.
- **`hd test --jobs N`:** every instance thread shares the `hd-io`
  thread and its one blocking pool. Each instance keeps its own reactor
  and inbox, so completions never cross instances.
- **REPL session:** the instance lives across inputs and so do its
  sockets (`session.rs` sets `close_on_end = false`). The runtime is
  process-wide, so no input owns it. `Host::restart` → `drop_all`
  (`host/state.rs:139-148`, `reactor.rs:350-353`) must now abort every
  pending task.
- **Rejected:** a multi-thread runtime. It sizes its worker pool to the
  core count and competes with `--jobs` instance threads, and
  `rt-multi-thread` compiles more code. If `hd-io` ever saturates, the
  change is one builder line (`new_multi_thread().worker_threads(2)`).
  A runtime per instance was also rejected: it needs either a thread per
  instance or `block_on` (see 4.1).
- **Blocking pool:** used for DNS (`tokio::net::lookup_host`), ureq
  requests and special-file `Fs`. Keep the default cap of 512 threads.
  A FIFO read that never completes keeps one pool thread until the
  process exits. `std-fs.special-file.deadline` allows that.

### 4.3 Timers, deadlines, abort and cancel

- **`Clock.sleep`** stays a reactor timer (`reactor.rs:133`). Offloaded
  operations have no `at`, as a pending read has today, so the wait's
  timeout is still the nearest timer or the program deadline.
- **`--time-limit` and test timeouts** stay in `Host::wait`. Providers no
  longer receive `host.deadline()`. The one exception is inline
  regular-file `Fs`, which keeps its checks before and after. When the
  wait returns `TimeLimit`, the trap path (`Control::Trap`) aborts every
  pending task through `drop_all`. Per-request timeouts stay inside the
  task: Http's 30 s default becomes ureq's `timeout_global`, and Net's
  250 ms per connect candidate becomes `tokio::time::timeout`.
- **Process:** spawn with `tokio::process::Command`, using
  `process_group(0)` and `kill_on_drop(true)` as a backstop. On abort,
  the host signals the task through a oneshot. The task calls
  `rustix::process::kill_process_group(pgid, KILL)` and then
  `child.wait().await`, so the child is reaped, not left to tokio's
  best-effort orphan reaper (`kill_on_drop` caveats, docs.rs). On
  Windows, killing the group needs a Job object (question Q1).
- **Http:** the request runs on the blocking pool. Abort calls
  `shutdown(Both)` on a clone of the request's `TcpStream`, kept in a
  cancel slot. ureq's blocked read then fails at once, and the
  connection is closed (`std-http.send.cancel.close`). ureq does not
  export its `TcpTransport` (`ureq-3.4.2/src/unversioned/transport/mod.rs:39-40`),
  so `HttpTcpConnector` (`http/connector.rs:114-180`) wraps its own
  `std::net::TcpStream` in a small `Transport` over ureq's
  `LazyBuffers`, about 80 lines.
- **Net:** `read`, `accept` and `recv_from` are cancel-safe in tokio
  (`AsyncReadExt::read`, `TcpListener::accept`, `UdpSocket::recv_from`
  docs, "Cancel safety"). So aborting the task leaves unread bytes in the
  kernel for the next call, which is `std-net.cancel.read` with no host
  buffer at all. `write` is not cancel-safe. `std-host.op.cancel.write`
  allows that. `close` aborts the handle's pending tasks, completes
  their operations with `Disposed` from the host side, and drops the
  socket (`std-net.close.*`).
- **Special-file Fs:** blocking `std::fs` on the blocking pool. Abort
  only drops the result (`std-fs.special-file.deadline`).

### 4.4 Journal and replay

- Nothing moves: the journal records at the import boundary
  (`host/link.rs:122-133`). Today `Net`, `Http` and `Process` `.start`
  rows hold the whole result (status 0). After the change, `.start`
  records `(1, handle)`, and the result is recorded in the `.finish` row
  (`Record::Finish`, `host/kind.rs:79-98`). The wake batch is recorded
  by `block` as now.
- Each capability job turns its `Kind::NeverFinish`
  (`host/kind.rs:222,249,251`) into a real finish kind.
- Replay serves `.start`, `.finish` and `block` from the record
  (`host/link.rs:210-248`) and never calls `call`, so no task is spawned
  (`cli.repl.host.once`). The recorded batch order makes a replay
  deterministic, even though live order is not (`std-host.op.no-order`).
- Journals live only in memory (`SESSION_CAP`), so there is no old
  format to migrate.

### 4.5 Heap accounting (S7)

- `HeapBudget.charged` (`limits.rs:47-81`) moves into a shared ledger,
  `Arc<AtomicU64>` plus the maximum. `memory_growing` and host tasks
  charge the same counter.
- A task reserves bytes before it buffers them: each Process output
  chunk, an Http body (ureq `with_config().limit(remaining)`, replacing
  the unbounded read at `http/request.rs:209-213`), a Net read buffer of
  `min(max, 64 KiB)`, and held write payloads and Process input.
- When a reservation fails, the task ends its work (killing the child,
  for example) and delivers `Done::HeapExhausted`. `.finish` turns that
  into the `heap-exhausted` panic (`std-host.op.heap.exhausted`).
- `.finish` releases the reservation just before `put_bytes` copies the
  payload into the exchange buffer. That growth is charged by
  `memory_growing`, so the bytes are never counted twice. Abort releases
  it too.
- The 1 GiB Process caps go (`core/process.rs:61`,
  `test_engine/process.rs:44`, `std-host.op.heap.no-other-cap`).

### 4.6 Http client: ureq or hyper

| Option | Crates (macOS) | Code | Cancel |
| --- | --- | --- | --- |
| **ureq on `spawn_blocking` (recommended)** | +0; ureq, ureq-proto and utf8-zero stay | keeps `http/request.rs`, `connector.rs`, redirect and grant logic; adds the ~80-line cancel-slot transport | `shutdown(Both)` on the cancel slot |
| hyper 1.x (`client`, `http1`) + tokio-rustls | about +8 (hyper, http-body, http-body-util, want, try-lock, atomic-waker, futures-core, tokio-rustls) and −3 (ureq, ureq-proto, utf8-zero); itoa, smallvec, bytes, http and httparse are already in the tree | rewrites about 1,000 lines: connector, candidate order, deadline transport, body framing | drop the future |
| reqwest | requires `url`, which brings back the 26 IDNA/ICU crates that S9 removes | | |

hyper's dependency list is from its manifest on docs.rs. It is not
built here, so its count is an estimate. Recommendation: keep ureq. A
blocking thread per in-flight request is fine at CLI scale. Revisit only
if a program needs hundreds of concurrent requests (question Q2).

### 4.7 Dependency delta

Today (from §2): 126 external crates on macOS and 129 on Windows.

| Change | macOS | Windows | Job |
| --- | --- | --- | --- |
| add `tokio =1.53.2`, `default-features = false`, features `rt`, `net`, `process`, `time`, `io-util`, `sync` | +4: tokio, mio, pin-project-lite, signal-hook-registry (Unix only); `bytes`, `libc`, `socket2`, `errno` and `windows-sys 0.61` are already in the tree | +3 | T1 |
| no `rt-multi-thread`, `macros`, `signal` or `fs` (the REPL keeps `ctrlc`; a select uses `std::future::poll_fn`) | 0 | 0 | |
| drop `url` (S9) | −26 | −26 | T10 |
| drop `nix` (rustix gains the `process`, `system`, `fs` features, with no new crates) | −1 | 0 | T12 |
| `hostname` becomes Windows-only: Unix uses `rustix::system::uname`; Windows has no safe std call | −1 | 0 | T12 |
| drop direct `libc` use (`net/socket.rs:148` and `fs/io.rs` go with the polling) | 0: tokio and mio use it | 0 | T12 |
| `socket2`: keep it direct only for dual-stack UDP `set_only_v6` and the listen backlog, which tokio's `TcpSocket` lacks | 0: tokio's `net` depends on it | 0 | T4 |
| drop `interprocess` (+ doctest-file, recvmsg, widestring) and `winapi-util` | 0 | −4 to −5 | T7 |
| **Estimated total** | **≈102 (−24)** | **≈101 (−28)** | |

Build time is not measured for tokio. Its features here are small, and
removing url+ICU saves about 24 s of summed crate time (§2). T1 and T12
report the measured `--timings` totals.

### 4.8 What is deleted

- The Net busy-poll worker: `net/state.rs:56-77` (serve),
  `:181-216` (connect loop), `:218-314` (`arrivals`, the unbounded
  inbox); `net/client.rs:211-279` (request and worker); the DNS helper
  thread `net/socket.rs:72-106`; and the 5 ms loops in `net/ops.rs`.
- The Process pipe threads: all of `core/workers.rs` (three threads per
  run plus 50 ms polling), plus `core/pipes.rs:158-260` (`wait_pipe`,
  `drain`, `feed_input`, `fcntl`) and the Windows pipe pairs
  (`core/pipes.rs:35-66`).
- FIFO and Windows-pipe polling: `fs/io.rs` `wait`, `wait_regular` and
  `pause`, the O_NONBLOCK and ENXIO open loop (`fs/io.rs:142-233`), and
  `windows_pipe.rs` (386 lines).
- The process-wide Http worker: `http/client.rs:92-159` and the
  unbounded `recv()` at `:68-72`.
- With S8 (T11): `http/transport.rs`, `http/chunk.rs` and most of
  `http/headers.rs`, about 900 lines.

### 4.9 Cost model

| Stage | Cost |
| --- | --- |
| Granularity | One task per operation: one allocation for the task, one mutex push and a condvar notify. Small next to the syscall it waits on |
| Contention | One inbox mutex per instance, touched by `hd-io` and the instance thread. One I/O thread for all `--jobs` instances; epoll/kqueue handles tens of thousands of events a second |
| Locality | A completion crosses one thread boundary. The payload `Vec` moves and is not copied until `put_bytes`, as today |
| Superlinear | `Reactor::due` scans every op on each wake (`reactor.rs:241-263`). It is O(pending) per wake, so 10k pending reads cost 10k checks per wake. T1 adds a ready list filled from the inbox, so `due` scans only timers and ready handles |
| I/O | No polling anywhere. Idle CPU drops from 200 wakeups a second (`net/state.rs:59`) to zero |
| Incremental | Lazy runtime: programs that make no offloaded call pay nothing |

## 5. Fix Plan

Each job is about an hour and has one concern. It runs focused dev
tests, then the tiered gates. "Opus" marks the hard jobs (new
concurrency or protocol). "Sonnet" marks the mechanical ones. Every job
removes its rows from the §3 table. Every impl job reports tiny-program
WAT, functions and build time before and after.

| Job | Model | Needs | Work | Done when |
| --- | --- | --- | --- | --- |
| F0 | | | Flush held Console output before host waits | done (0b6309e9); flush probe green |
| T1 | opus | | Add tokio (features of §4.7) and the lazy `hd-io` runtime. Generalize the reactor inbox to `(handle, Done)`, with `Reactor::offload(future) -> handle`, a stored abort handle, a ready list for `due`, and `drop_all` and `Control::Abort` aborting tasks. No capability moves yet. Mark engines-and-test-runner.md §20.6 Q1 and runtime-and-host.md §17.5 as superseded by §4 here | done in "Add the lazy hd-io tokio runtime and offloaded reactor ops"; external crates 126 → 130 on macOS, 129 → 132 on Windows. `Reactor::offload` stays test-only until T2 gives it a caller |
| T2 | opus | T1 | `Process.run!` on `tokio::process`: own process group, concurrent stdin/stdout/stderr, abort kills the group and reaps, real `run.finish` kind. Delete `core/workers.rs` and the pipe parts of `core/pipes.rs` | probe 2 takes ~1 s in both orders; `race!(run!("sh", ["-c", "sleep 5 & sleep 5"]), sleep!(100ms))` returns at ~0.1 s and `pgrep` finds no `sleep`; a REPL Ctrl-C during `run!` prints `cancelled`; a replayed session input with `run!` spawns nothing |
| T3 | sonnet | T2 | `std-process.child.signal`: status is 128 + `ExitStatusExt::signal()` on Unix (`core/pipes.rs:145,149`) | `run!("sh", ["-c", "kill -TERM $$"])` reports status 143; a normal exit is unchanged |
| T4 | opus | T1 | `Net` on tokio: handle table on the host side with tasks per operation (lookup, connect with Node family order and a 250 ms per-candidate timeout, listen, bind_udp, read, write, accept, send_to, receive), real finish kinds. Delete the worker, `arrivals` and the DNS thread. `close` keeps today's meaning until T5 | probe 3 prints `timeout` at ~0.5 s; `all!(accept!, connect!)` on loopback completes in one program; cancelling `read!` and then reading again returns the first bytes (`std-net.cancel.read`); a REPL Ctrl-C during `accept!` prints `cancelled`; an idle program with an open listener uses no CPU |
| T5 | sonnet | T4 | `std-net.close.release` and `std-net.close.pending`: `close` aborts the handle's pending tasks, completes them with `Disposed`, and drops both halves | after `close`, the open-descriptor count (`lsof -p`, or `/proc/self/fd`) is back to baseline and the peer reads EOF at once; `race!` over a pending `read!` and a `close` yields `Disposed` |
| T6 | opus | T1 | `Http.send!` as a blocking-pool task with a cancel-slot transport; abort calls `shutdown(Both)`; the request timeout stays in ureq; real `send.finish` kind. Delete the shared worker | `race!(send!(slow local server), sleep!(100ms))` returns at ~0.1 s and the server reads EOF; two `send!` to a 500 ms server under `all!` take ~0.5 s; two `hd test --jobs 2` instances no longer queue behind each other |
| T7 | sonnet | T1 | Special-file `Fs`: `.start` stats the path. A regular file or directory runs inline as today. Anything else (FIFO, device, Windows named pipe) runs blocking `std::fs` on the blocking pool. Delete the polling in `fs/io.rs`, `windows_pipe.rs`, `interprocess` and `winapi-util` | `race!(read_bytes!(fifo with no writer), sleep!(100ms))` returns at ~0.1 s; a FIFO with a writer reads fully; regular-file fs tests are unchanged; the Windows build compiles without `interprocess` |
| T8 | sonnet | T1 | Test-engine child programs (`test_engine/process.rs:72`): run the child instance on its own big-stack std thread and deliver into the inbox. It cannot be a tokio task, since it needs a big stack | an `hd test` case with `all!(run!(child), sleep!(1s))` against a 1 s child takes ~1 s; the existing test-engine process tests pass |
| T9 | opus | T2, T4, T6 | S7 heap ledger (§4.5): shared counter, reservations in the Process, Http and Net tasks, release at `.finish` or abort, `heap-exhausted` panic. Remove both 1 GiB caps | a body over `--max-heap 64MiB` panics `heap-exhausted`; a child printing 100 MiB under a 64 MiB budget panics `heap-exhausted` and is killed; no `1 << 30` remains; limits tests pass |
| T10 | sonnet | T6 | S9: drop `url`. Parse with `http::Uri` (absolute `http`/`https`, ASCII host, else `InvalidUrl`), grant on the lowercase host, and write the RFC 3986 §5.2 join for redirects | `cargo tree` loses 26 crates; `http://bücher.example/` is `InvalidUrl`; redirect and grant tests pass |
| T11 | sonnet | T10 | S8: drop the raw-header transport (`http/transport.rs`, `chunk.rs`, most of `headers.rs`) and use ureq's request and response headers | files deleted; http tests compare header names ignoring case; values of one name keep their order |
| T12 | sonnet | T2, T4, T7 | S6/F10: drop `nix` (rustix `poll`/`fcntl`/`mkfifo` in tests, `kill_process_group`), Unix `hostname` (`rustix::system::uname`; Windows keeps the crate) and direct `libc` use; keep `socket2` only where tokio lacks the option | `cargo tree -p hd_run_wasmtime --depth 1` lists no `nix` or `libc`, and `hostname` only for Windows; the macOS external count is ≈102; `Sys.hostname` matches `hostname(1)` |

Order: T1, then T2 and T3, then T4 and T5, T6, T7, T8, T9, T10, T11,
T12. The pairs T2/T3 and T4/T5 touch the same files back to back. T9
waits for the three tasks it charges. T10 and T11 follow T6 because all
three edit `http/request.rs`.

### 5.1 Acceptance probes

These are the three probes from the top of this audit.

| Probe | Expected | Turned green by |
| --- | --- | --- |
| `println("before")`, `sleep!(1.5s)`, `println("after")` | `before` shows 1.5 s ahead of `after` | F0 (done) |
| `all!(run!("sleep", ["1"]), sleep!(1s))`, both orders | ~1 s | T2 |
| `race!(listener.accept!(), sleep!(500ms))`, `--time-limit 4s` | `timeout` at 0.5 s | T4 |

### 5.2 Risks

- **Late completions.** A task can deliver after its abort. The inbox is
  keyed by handle, and `take_delivered` drops a handle with no op (T1
  test). Handles wrap at `i32` (`reactor.rs:113-125`). A delivery from
  a dropped op could reach a reused handle only after 2^32 starts.
  Accepted.
- **PID reuse on group kill.** The task kills the group only before it
  reaps the leader, so the pgid cannot be reused while the kill can
  happen (T2).
- **Stuck blocking threads.** A FIFO with no writer, or a DNS lookup that
  never returns, keeps a pool thread. This is bounded by the 512 cap and
  allowed by `std-fs.special-file.deadline`.
- **A runtime in `cargo test`.** The process-wide `hd-io` is shared by
  every test in the binary, as `hd-epochs` is. Tests must not assume a
  fresh runtime.

### 5.3 Questions for the owner

| # | Question | Recommendation |
| --- | --- | --- |
| Q1 | On Windows, `std-process.cancel` asks to kill the child's process group. That needs a Job object, which means `unsafe` FFI. The crate is `#![forbid(unsafe_code)]`, and the only approved `unsafe` is in `hd_cli` and `hd_web`. The other way is a new crate. | Kill the direct child only on Windows for now (tokio `kill`), and keep a §3 row for the gap. Revisit when Windows CI runs the process tests. |
| Q2 | Keep Http on ureq, with a blocking-pool thread per in-flight request, or move to hyper on tokio? | Keep ureq (§4.6): no new crates, and the redirect, grant and candidate code stays. hyper adds about 8 crates and rewrites about 1,000 lines for concurrency the CLI does not need. |

Not in this plan: the Node host's `race!` hang past `--time-limit` (probe
above) belongs to the Node reference, which is being retired.
