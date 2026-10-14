# Wasmtime Host Audit (#160, step 1)

Status: audit only, 2026-10-13. No code, spec or fixture changed. The
owner reviews this before any fix job or std-spec edit.

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
- Console output is held in memory until exit, a stderr write or a
  `read_line!`. A program that prints and then waits shows nothing.
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
| `Console.write_line!` | completes at once in the reactor (`reactor.rs:128`) | main; bytes go to `Host::out` | not applicable | n/a | n/a | OK, but output is held until exit, stderr or `read_line!` (`host/state.rs:242-260`; flush probe) |
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
| `nix` | `poll`/`select` for FIFOs and process pipes, `fcntl`, test `mkfifo` | 4 | 1 | `rustix` (already in the tree, 0 new crates) covers poll, fcntl and mkfifo; the FIFO polling may go entirely (jobs F6, F10) |
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

Async runtimes stay out: tokio (with mio) and hyper or reqwest would
only add crates. The fix plan below needs none of them.

## 3. Spec

### Divergences

| Rule | wasmtime behavior |
| --- | --- |
| [`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort), [`std-http.send.cancel`](../../spec/std/http.md#r-std-http.send.cancel) | met only because nothing is ever pending; `race!` cannot cancel a request, a child process or a socket wait |
| [`req.waker.arrange`](../../spec/lang/11-requirements-and-suspension.md#r-req.waker.arrange) with [`req.schedule.all-unfinished`](../../spec/lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished) | `all!` children run one after another: the first blocking start holds the rest |
| [`cli.repl.cancel`](../../spec/cli/command-line.md#r-cli.repl.cancel) (Stop while waiting on the host runs `defer`) | a Stop during a blocking host call is seen after the call returns and reports `stopped`, not `cancelled` (`host/call.rs:401`, `instance.rs:269-295`) |
| [`module.process.other`](../../spec/lang/10-modules.md#r-module.process.other) ("cannot start") | output over 1 GiB after the child started becomes `Other` (`core/workers.rs:121-123`, `core/process.rs:72-83`) |

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

One implementation-design question (not std spec): **D1.** Replace the
sync-only choice of engines-and-test-runner.md §20.6 Q1 with "every
waiting `.start` returns a handle; the work runs on a thread and
completes into the reactor". Recommended. It is Go's model for blocking
syscalls, needs no tokio, and keeps most provider code. The reactor in
runtime-and-host.md §17.5 (tokio) stays rejected for its dependency
cost.

## 4. Fix Plan

Each job is about an hour, one concern, with focused dev tests. Jobs
F1 to F6 need D1 approved. Jobs that depend on a question say so.

| Rank | Job | Done when |
| --- | --- | --- |
| F0 | Flush held Console output before every host wait (`Host::wait`, `block`) and past a size bound | flush probe shows `before` 1.5 s ahead of `after`; conformance output bytes unchanged |
| F1 | Reactor completion inbox: any thread delivers `(handle, encoded result)`; `.finish` returns it; `abort` drops a late result; the journal records it at `.finish` | reactor tests: two offloaded ops complete out of order, an aborted op's late result is dropped, a Stop wakes the wait; session replay tests pass |
| F2 | `Net` async: the worker replies into the reactor and holds several pending operations at once instead of serving one at a time | `race!(accept!, sleep!(500ms))` prints `timeout` at ~0.5 s; `all!(accept!, connect!)` on loopback completes in one program; REPL Ctrl-C during `accept!` prints `cancelled` |
| F3 | `Process.run!` async: run the existing code on a thread with a cancel flag; cancel kills the child | `all!` probe takes ~1 s in both orders; `race!(run!("sleep", ["5"]), sleep!(100ms))` returns at ~0.1 s and no `sleep` process remains |
| F4 | `Http.send!` async: a thread per request instead of the process-wide worker; abort closes the connection | `race!` against a slow local server returns at the sleep and the server sees the close; two parallel `send!` to a 500 ms server take ~0.5 s under `all!` |
| F5 | `Net` cleanup: full close that removes the handle; read only for a pending read or up to a small per-socket bound; wait with `poll` instead of 5 ms sleeps | after `close`, the descriptor count returns to baseline; a peer sending 1 GiB to an unread socket does not grow host memory past the bound; an idle worker uses no CPU |
| F6 | `Fs` async: run each operation on a small thread pool | `race!(read_bytes!(fifo with no writer), sleep!(100ms))` returns at ~0.1 s; fs tests pass |
| F7 | (S7) Count Http bodies, Process output and Net buffers against the heap budget | a body over `--max-heap` panics `heap-exhausted`; the 1 GiB constant is gone |
| F8 | (S9) Drop `url`: parse with `http::Uri`, write the RFC 3986 join | `cargo tree` loses 26 crates; redirect and grant tests pass |
| F9 | (S8) Drop the raw-header transport; use ureq's response | `http/transport.rs` and `chunk.rs` deleted; http tests updated to case-insensitive names |
| F10 | (S6, after F5/F6) Drop FIFO and Windows-pipe polling; replace `nix`, `libc`, `socket2`, `hostname` with `rustix` and std | `cargo tree --depth 1` lists none of them; Windows build lists no `interprocess` or `winapi-util` |

Not in this plan: the Node host's `race!` hang past `--time-limit` (probe
above) belongs to the Node reference, which is being retired.
