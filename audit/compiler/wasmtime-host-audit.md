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
| `FsRead`, `FsWrite` | inline for a regular file or a directory; suspends since T7 for a special file: `.start` stats the path (`fs/api.rs`, `start`), and a FIFO, device or Windows named pipe is one `hd-io` blocking-pool task of plain `std::fs` (`Job::run`), with a real finish kind | main for regular files and directories (owner-approved, §4); `hd-io` blocking-pool thread for special files | run, for a special file (probe: `race!` of a FIFO read with no writer against `sleep!(100ms)` returns the sleep) | a regular file: checked before and after only; a special file: a `time-limit` trap ends every pending operation (`Control::Trap` → `drop_all`) | abort drops the result only. The blocking thread cannot be cancelled and lingers until its I/O ends (a read of a FIFO until a writer closes it), which `std-fs.special-file.deadline` allows; a REPL Stop prints `cancelled`. A slow disk or NFS on a regular file still blocks the program | OK for special files; regular files stay inline |
| `Process.run!` | suspends since T2: `.start` checks the grant and arguments, then one `hd-io` task spawns the child in its own process group and handles its three pipes together (`core/process.rs`) | `hd-io` thread | run (probe: `all!` 1.07 s in both orders) | a `time-limit` trap ends every pending operation (`Control::Trap` → `drop_all`), which kills the child's group | abort kills the process group (Unix) or the Job object (Windows), then reaps the child; a REPL Stop prints `cancelled` | OK |
| `Http.send!` | suspends since T6: `.start` decodes the request and checks its URL, grant and timeout on the main thread; otherwise one `hd-io` task sends it with ureq on the runtime's blocking pool (`http/client.rs`) | `hd-io` blocking-pool thread, one per request in flight; concurrent instances (`hd test --jobs`) no longer queue | run (probe: `all!` of two `send!` to a 500 ms server takes 0.55 s) | the request timeout stays in ureq (`timeout_global`, capped per socket wait by the whole-request deadline); a `time-limit` trap ends every pending operation (`Control::Trap` → `drop_all`) | abort drops the task, which shuts the connection's socket down both ways through the request's cancel slot (`http/connector.rs`, `Cancel`): the blocked request ends at once and the server reads EOF (probe: `race!` with a 100 ms sleep prints `timeout`, EOF 0.105 s after the request); a REPL Stop prints `cancelled` | OK |
| `Net` (all methods) | suspends since T4: `.start` checks the grant, the handle and what the socket already holds (bytes a cancelled read gave back, a sticky end) on the main thread; anything that waits is one `hd-io` task per operation over tokio sockets (`net/ops.rs`), and `.finish` puts a new socket in the instance's handle table (`net/client.rs`) | `hd-io` thread; DNS on its blocking pool | run (probe 3: `timeout` at 0.5 s) | a `time-limit` trap ends every pending operation (`Control::Trap` → `drop_all`) | abort drops the task; `read`, `accept` and `receive` take nothing until they complete, and a result delivered before the abort is given back (`std-net.cancel.read`); a REPL Stop prints `cancelled` | OK |
| `TestRunner` / `PropertyRunner` | plain, small fs work | main | n/a | checked after `snapshot_check` and `start` | n/a | OK |

Other `Net` findings, independent of blocking:

| Finding | Pointer | Effect |
| --- | --- | --- |
| `TcpStream.close` only shuts down the write half and keeps the socket | `net/ops.rs:30-33` | the descriptor stays open until the instance ends |
| Fixed by T4: `arrivals` read every socket, closed ones included, into an unbounded inbox | was `net/state.rs:218-252` | a socket is read only by a pending `read!` or `receive!`, at most its `max` (64 KiB at most), so TCP flow control pushes back |
| Fixed by T4: the worker woke every 5 ms while it existed, even idle | was `net/state.rs:59` | no polling: an idle program waits in `kevent`/`epoll` |

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
| `interprocess`, `winapi-util`, `windows-sys` | Windows named-pipe paths in `Fs` and pipe handles for `Process` (`windows_pipe.rs`, 386 lines) | Windows only | 3 + deps | removed by T7 (`windows_pipe.rs`, `interprocess`, `winapi-util` and its helpers `doctest-file`, `recvmsg`, `widestring` deleted); `windows-sys` stays for error codes and `FILE_FLAG_BACKUP_SEMANTICS` |
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
| [`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort) | met for `Process`, `Net`, `Http` ([`std-http.send.cancel`](../../spec/std/http.md#r-std-http.send.cancel)) and an `Fs` call on a special file, which suspend and abort (T2, T4, T6, T7); an `Fs` call on a regular file or a directory runs inline (owner-approved, §4), so `race!` cannot cancel it |
| [`req.waker.arrange`](../../spec/lang/11-requirements-and-suspension.md#r-req.waker.arrange) with [`req.schedule.all-unfinished`](../../spec/lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished) | `all!` children run one after another: the first blocking start holds the rest |
| [`cli.repl.cancel`](../../spec/cli/command-line.md#r-cli.repl.cancel) (Stop while waiting on the host runs `defer`) | a Stop during a blocking host call is seen after the call returns and reports `stopped`, not `cancelled` (`host/call.rs:401`, `instance.rs:269-295`) |
| [`module.process.other`](../../spec/lang/10-modules.md#r-module.process.other) ("cannot start") | output over 1 GiB after the child started becomes `Other` (`core/process.rs`, `drain` and `finish`) |

Std rules written from S1-S12 (owner-approved 2026-10-13) that the
wasmtime host does not meet yet. `KNOWN_FAILURES.tsv` cannot list them:
it holds parse-phase fixture paths only, and the corpus test fails on a
row it does not see. Each fix job removes its rows here.

| Rule | wasmtime behavior | Fix job |
| --- | --- | --- |
| [`std-host.op.wait`](../../spec/std/host.md#r-std-host.op.wait) | test-engine child programs block the whole instance (`Process` suspends since T2, `Net` since T4, `Http` since T6, special-file `Fs` since T7) | T8 |
| [`std-process.cancel`](../../spec/std/process.md#r-std-process.cancel) | met since T2. On Windows the abort closes the child's Job object (`win32job`, the owner's answer to Q1). A process the child starts before the host assigns it to the job, right after spawn, is outside the job and survives | Windows window: open |
| [`std-host.op.heap`](../../spec/std/host.md#r-std-host.op.heap), [`std-host.op.heap.exhausted`](../../spec/std/host.md#r-std-host.op.heap.exhausted), [`std-host.op.heap.no-other-cap`](../../spec/std/host.md#r-std-host.op.heap.no-other-cap) | `Http` bodies, `Process` output and `Net` inboxes are outside `--max-heap`; `Process` output has its own 1 GiB cap | T9 |
| [`std-console.host.visible`](../../spec/std/console.md#r-std-console.host.visible) | met: flushed before every host wait and past 64 KiB (F0 done) | F0 |
| [`std-http.url.non-ascii`](../../spec/std/http.md#r-std-http.url.non-ascii), [`std-http.url.no-normalize`](../../spec/std/http.md#r-std-http.url.no-normalize) | `url` maps a non-ASCII host through IDNA instead of returning `InvalidUrl` | T10 |
| [`std-process.child.signal`](../../spec/std/process.md#r-std-process.child.signal) | met since T3: a signal-killed child reports 128 plus the signal number on Unix (`status_value` in `core/process.rs`). Windows has no signal status. The Node host still reports a flat 128, so the parity test no longer compares this case | done in "Report 128 plus the signal number for a signal-killed child" |

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
  Windows the child is assigned to a kill-on-close Job object
  (`win32job`, the owner's answer to Q1), and the abort closes it.
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
| add `win32job =2.0.3` (Windows only; Q1): it brings `windows 0.61` and its family (windows-core, -collections, -future, -implement, -interface, -numerics, -result, -strings, -threading, windows-link 0.1) and `thiserror 1` | 0 | +14 | T2 |
| **Estimated total** | **≈102 (−24)** | **≈115 (−14)** | |

Build time is not measured for tokio. Its features here are small, and
removing url+ICU saves about 24 s of summed crate time (§2). T1 and T12
report the measured `--timings` totals.

### 4.8 What is deleted

- Done in T4: the Net busy-poll worker: `net/state.rs:56-77` (serve),
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
- Done in T6: the process-wide Http worker (`http/client.rs:92-159`),
  the unbounded `recv()` at `:68-72`, and ureq's `TcpConnector` with the
  `DeadlineTransport` wrapper around it (`http/connector.rs`), which the
  cancel-slot `SocketTransport` replaces.
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
| T2 | opus | T1 | `Process.run!` on `tokio::process`: own process group, concurrent stdin/stdout/stderr, abort kills the group and reaps, real `run.finish` kind. Delete `core/workers.rs` and the pipe parts of `core/pipes.rs` | done in "Run Process.run! as an hd-io task that kills its group on abort": probe 2 takes 1.07-1.10 s in both orders; the `race!` probe prints `timeout` at 0.15 s and `pgrep` finds no `sleep 5`; a REPL Stop prints `cancelled` and the child is gone; a rebuilt session runs no child again. An abort returns once its task is dropped, and a trap ends every pending operation. Windows uses a Job object (+14 crates, Q1) |
| T3 | sonnet | T2 | done in "Report 128 plus the signal number for a signal-killed child". `std-process.child.signal`: status is 128 + `ExitStatusExt::signal()` on Unix (`status_value`, `core/process.rs`) | `run!("sh", ["-c", "kill -TERM $$"])` reports status 143; a normal exit is unchanged |
| T4 | opus | T1 | `Net` on tokio: handle table on the host side with tasks per operation (lookup, connect with Node family order and a 250 ms per-candidate timeout, listen, bind_udp, read, write, accept, send_to, receive), real finish kinds. Delete the worker, `arrivals` and the DNS thread. `close` keeps today's meaning until T5 | done in "Run Net as hd-io tasks over a host-side socket table": probe 3 prints `timeout` at 0.56 s (process wall time); `all!(accept!, connect!)` on loopback completes in 0.08 s; a `race!`-cancelled `read!` leaves the bytes, and the next read returns them; a REPL Stop during `accept!` prints `cancelled`; a rebuilt session connects no second time; an idle program waiting in `accept!` stays at 0.10 s of CPU over 2 s. External crates unchanged (130 macOS, 146 Windows): `socket2` stays for the backlog, dual-stack UDP and `SockRef::shutdown`. The Windows build is not checked locally |
| T5 | sonnet | T4 | `std-net.close.release` and `std-net.close.pending`: `close` aborts the handle's pending tasks, completes them with `Disposed`, and drops both halves | done in "Close a socket at once and end its pending operations": `close` shuts a stream both ways, ends every operation not yet finished on the handle with `Disposed` (task aborted and awaited, so the socket drops before `close` returns), and the peer reads EOF at once. Verified with `lsof -p`: after the closes only the program's own open sockets remain; `race!` over a pending `read!` and a delayed `close` prints `Disposed`; a closed listener ends its pending `accept!` with `Disposed` and its port can be listened on again. A UDP socket ends a pending `receive!` the same way, which the spec states only through `std-net.handles.closable`. The Node host now destroys a closed stream instead of ending its write half |
| T6 | opus | T1 | `Http.send!` as a blocking-pool task with a cancel-slot transport; abort calls `shutdown(Both)`; the request timeout stays in ureq; real `send.finish` kind. Delete the shared worker | done in "Run Http.send! as an hd-io task that closes its connection on abort": `race!(send!(silent local server), sleep!(100ms))` prints `timeout`, and the server reads EOF 0.105 s after the request; `all!` of two `send!` to a 500 ms server takes 0.55 s (process wall time); two cases of one `hd test --jobs 2` program reach a 500 ms server together, and two engine instances in one test send at the same time; a REPL Stop during `send!` prints `cancelled` and the server reads EOF; a rebuilt session sends no request again. External crates unchanged (130 macOS). On Windows, whether `shutdown` wakes a blocked `recv` is not checked locally |
| T7 | sonnet | T1 | Special-file `Fs`: `.start` stats the path. A regular file or directory runs inline as today. Anything else (FIFO, device, Windows named pipe) runs blocking `std::fs` on the blocking pool. Delete the polling in `fs/io.rs`, `windows_pipe.rs`, `interprocess` and `winapi-util` | done in "Run Fs special files as hd-io tasks on the blocking pool": `race!` of a FIFO read with no writer against `sleep!(100ms)` returns the sleep; a FIFO with a writer reads 200 000 bytes whole; a regular file or directory runs inline and starts no `hd-io` thread (`tests/io_runtime.rs`, `io_started`); a REPL Stop during a blocked FIFO read prints `cancelled`; a rebuilt session reads no FIFO again. `cargo tree` on Windows: 207 → 202 distinct lines (`interprocess`, `winapi-util`, `doctest-file`, `recvmsg`, `widestring` gone); macOS unchanged (130 external). The Windows lint passes. Not checked: a real named pipe on Windows. A pipe is found by its name (`\\.\` device paths and UNC `pipe` shares), since std cannot tell a pipe from a file without opening it |
| T8 | sonnet | T1 | Test-engine child programs (`test_engine/process.rs:72`): run the child instance on its own big-stack std thread and deliver into the inbox. It cannot be a tokio task, since it needs a big stack | an `hd test` case with `all!(run!(child), sleep!(1s))` against a 1 s child takes ~1 s; the existing test-engine process tests pass| done in "Run test-engine children on their own threads, off the call": each child instance runs on its own big-stack `hd-test-process` thread (`thread_stack`, as before), and a task on `hd-io` waits for its result and delivers it as `Done::Child`, so `.start` returns `(1, handle)` and `ProcessFinish` records the result. Abort drops the wait, which sets the child host's `interrupt` flag (checked at host calls, in waits and at epoch ticks), so the thread ends promptly; its late result is dropped. Two 1 s children finish together in 1.1 s, and an aborted busy child's thread ends. Replay serves the finish row and runs no child |
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
| Q1 | On Windows, `std-process.cancel` asks to kill the child's process group. That needs a Job object, which means `unsafe` FFI. The crate is `#![forbid(unsafe_code)]`, and the only approved `unsafe` is in `hd_cli` and `hd_web`. The other way is a new crate. | Answered 2026-10-14: add a Job-object crate. T2 uses `win32job =2.0.3` (released May 2025). No Job-object crate on `windows-sys` was found: `process-wrap` and `kill_tree` also use `windows`, and `command-group` uses `winapi`. |
| Q2 | Keep Http on ureq, with a blocking-pool thread per in-flight request, or move to hyper on tokio? | Keep ureq (§4.6): no new crates, and the redirect, grant and candidate code stays. hyper adds about 8 crates and rewrites about 1,000 lines for concurrency the CLI does not need. |

Not in this plan: the Node host's `race!` hang past `--time-limit` (probe
above) belongs to the Node reference, which is being retired.

### 5.4 Findings after T4

| Finding | Effect | Fix |
| --- | --- | --- |
| macOS abort on a handled signal (`mach_msg failed with 268451845`) | wasmtime's Mach-port exception thread waits with `MACH_RCV_INTERRUPT` and calls `abort()` when a signal interrupts it (`wasmtime-49.0.2/src/runtime/vm/sys/unix/machports.rs`, `handler_thread`). tokio's process driver handles SIGCHLD since T2, and `hd_cli` handles SIGINT (`ctrlc`). Any child exit or Ctrl-C that the kernel gave to that thread killed the process: the parity binary failed 2 runs in 16. `tests/child_signals.rs` routes SIGCHLD to that thread and aborted 30 runs in 30 | `wasmtime_config` sets `macos_use_mach_ports(false)`: guest faults use signal handlers, as on Linux, and no helper thread exists. The trap handler is process-wide, so the bare `Config` in `limits_tests.rs` sets it too. After: `child_signals` passes 30 in 30, the parity binary 16 in 16. `guard_page_fault_is_a_trap` checks that a linear-memory fault is still a trap. Cost: a Mach-port crash reporter such as Breakpad would see guest faults before wasmtime does and report them as crashes; the CLI links none |
