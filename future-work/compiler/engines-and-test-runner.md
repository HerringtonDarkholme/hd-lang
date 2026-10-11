# New Compiler Design: Engines And Test Runner

Part of the [compiler design](README.md).

## 18. Execution Engines And Tiers

### 18.1 wasmtime Configuration

The columns are the two pipelines of [tiering.md](tiering.md) (owner,
2026-10-07). `hd run`, `hd test` and the REPL use the dev pipeline;
`--release` selects the optimized one, also for `hd test --release`,
which keeps the test profile's checks.

| Setting | Dev pipeline | Optimized pipeline |
| --- | --- | --- |
| `wasm_gc`, `wasm_function_references` | on | on |
| exceptions, threads, stack switching | off | off |
| compiler | Cranelift `OptLevel::None` with the single-pass register allocator, if spike T1 confirms it; else the fallback of §18.6 | Cranelift, `OptLevel::Speed`, backtracking allocator |
| collector | the default copying collector | same |
| initial GC heap | `hd run`: from the profile, default 64 MiB, decided by E9; `hd test`: the pooling allocator's small per-slot heaps, sized by `unit-test-perf` (§18.3) | the same, from the profile |
| `epoch_interruption` | on | on |
| `consume_fuel` | off (fuel is for simulation, Later) | off |
| `max_wasm_stack` | the profile's stack limit | same |
| incremental compilation cache | on, through `CacheStore` | on |
| allocation strategy | pooling (§18.3) | pooling for `hd test`; on-demand for one `hd run` |
| parallel compilation | on, capped at the `--jobs` budget | same |

Winch cannot be the dev tier: it supports no GC. Both pipelines use
Cranelift, as the research found.

### 18.2 Compile Caches

- **Precompiled modules.** `Engine::precompile_module` output is stored
  as a `cwasm` entry. A disk entry is loaded with
  `Module::deserialize_file`, which maps the file instead of copying it.
  The key holds the Wasm hash, the wasmtime version, the engine
  configuration and the target, so a config change misses.
- **A `cwasm` is trusted native code (Codex re-review N-B7).**
  `deserialize_file` is unsafe: wasmtime runs the bytes as machine code,
  and neither a checksum nor the Wasm sandbox makes a foreign file safe.
  So `cwasm` entries live only in a private native-code directory,
  `$HD_CACHE/obj/native/`, created with owner-only permissions and
  refused if another user can write it. They are never shared between
  users and never fetched from a remote cache; a remote cache, if one
  comes later, carries Wasm only. The common framed entry format (cache.md
  §5.4) does not fit `deserialize_file`, which wants the raw artifact, so
  a `cwasm` is stored raw, published by the same write-then-rename, with
  a small framed sidecar entry that holds its key and checksum. The
  checksum is checked when the file is loaded, at about 10 GB/s.
- **Per-function cache.** `Config::enable_incremental_compilation` takes
  a cache store; an adapter maps it onto `CacheStore` as entry kind
  `cranelift`, and reads and writes it only through `clpack`s, one per
  folder group of the program (cache.md §5.4). No file holds one
  function. A program whose other functions did not change
  recompiles only the changed ones. The key is the function's meaningful
  CLIF, which abstracts direct call targets
  ([wasmtime#4155](https://github.com/bytecodealliance/wasmtime/issues/4155))
  but not global indices, type immediates or constants. So emitted bodies
  hold no program-wide dense number besides call targets (stable
  numbering, codegen.md §13.8): no site immediates, getters for lazily
  initialized globals, and content hashes for key ids and type ids.
  Without that, one new panic site in a debug build missed the cache for
  about half the program. S2 of spike 0c measures each remaining index
  space (types, constant globals, `ref.func`) after insertions; S3
  measures what a cache hit costs, since wasmtime still translates every
  function to compute its key. If a hit costs more than 40 percent of a
  compile, splitting a program into one module per module group is
  raised as an owner question.

### 18.3 Instantiation

- **One `Linker` per host kind** (default profile, unit test, integration
  test), built once per process.
- **One `InstancePre` per module**: imports resolved once, so each
  instantiation skips name lookup.
- **The pooling allocator** reserves slots for instances, exchange
  buffers, tables and GC heaps up front. Its totals scale with the worker
  count. An instantiation then takes a slot instead of mapping memory, a
  few microseconds.
- **A `Store` per run or per test case**, with its own `ResourceLimiter`
  and epoch deadline. Dropping the store returns the slot.

### 18.4 The Browser Runner

- The compiler worker builds the module (§20.6) and sends the bytes, the
  `hd.runtime` section and the source map request to the program worker.
- The program worker compiles with `WebAssembly.compile`, instantiates
  with the generated glue's imports, and runs the poll and wake loop on
  its event loop. V8 starts every function in Liftoff, which suits short
  playground programs.
- **Stop** or a timeout terminates the program worker only.
- **Conformance in a headless browser** (§21.2): a page hosts the program
  worker, and a Node driver feeds it the portable runtime cases, as the
  playground's e2e test drives a page today.

### 18.5 The Reserved Hot-Reload Table

Hot reload is Later. The emitter reserves its hook now
([Q14](research.md#q14-hot-reload-later-brief)):

- an emitter switch routes every user-to-user call through one `funcref`
  table, one slot per user function, in content order;
- the type section is already canonical and deterministic (§15.3), so
  the next build gives the same layouts the same Wasm types;
- with the switch on, module storage globals are exported.

The switch is off in the first release. Turning it on changes the
`pipeline_hash` part of `code_key` and nothing else.

### 18.6 Measured Choices

These engine settings are decided by measurements, not here:

- the dev pipeline's Cranelift setting, by spike T1
  ([tiering.md §7.2](tiering.md#72-spike-0c-additions); owner,
  2026-10-07: fast dev builds with poor code allowed). The dev pipeline
  uses `None` with the single-pass allocator if that saves at least 30
  percent of Cranelift CPU, passes every runtime conformance case, and
  keeps dev code within `dev-speed` (at most 4x the optimized pipeline,
  geomean, no case over 10x). Else it uses `None` with the backtracking
  allocator if that saves at least 10 percent; else `Speed`. The earlier
  rule, that debug moves to `Speed` if it costs more than 1.3x release,
  is retired with `release-check-cost`. Caching, stable numbering, packs
  and filtered test programs remain the latency levers in any pipeline;
- the initial GC heap of `hd run`: default 64 MiB, decided by E9.
  wasmtime's copying collector decides growth against the whole heap, so
  a long-lived set near the semi-space size is re-copied on every
  collection; one report measured 337 collections and an 8.8x slowdown,
  fixed by a larger initial heap. `--max-heap` still caps it;
- a null collector for short test instances (the research's experiment):
  adopted only if it measurably cuts `unit-test-perf`.

### 18.7 As Built (W1, #243): wasmtime Runs `hd run`

Owner, 2026-10-10: the `hd` CLI must not need Node. W1 links wasmtime
and runs programs in-process behind a switch; Node stays the default.

- **Engine.** `wasmtime = "=49.0.2"` (the newest release; the feature
  audit is `audit/compiler/wasmtime-audit-2026-10-10.md`), with default
  features off and `cranelift`, `gc`, `gc-copying`,
  `parallel-compilation`, `runtime` and `std` on: no component model, no
  async, no WASI, no pooling allocator, no `cwasm` cache yet. The
  `Config` of `hd_run_wasmtime::WasmtimeEngine::new`: `wasm_gc`,
  function references, reference types, multi-value, bulk memory and
  extended-const on; tail calls, exceptions and stack switching off; the
  copying collector with a 64 MiB initial GC heap; epoch interruption on
  (each store's deadline is far away until `--time-limit` exists);
  `max_wasm_stack` 4 MiB, on a thread of its own with 12 MiB of native
  stack; parallel compilation. The dev pipeline is Cranelift
  `OptLevel::None` with the backtracking allocator (spike T1 has not
  chosen the single-pass one, which wasmtime calls not production-ready);
  `--release` selects `OptLevel::Speed`.
- **Switch.** `hd run --engine wasmtime`, `hd FILE.hd --engine wasmtime`
  and `hd FILE.wasm --engine wasmtime`, or `HD_ENGINE=wasmtime` for any
  of them; `--engine node` or no setting keeps Node. The flag is a
  development switch and is not in `hd help`. Grants (`--cap`, the
  `[capabilities]` table) are resolved and the startup refusal checked
  before either engine runs, and the wasmtime providers ask the same
  `hd_run::Grants`.
- **Host.** `hd_run_wasmtime/src/host.rs` ports `compiler/host/core.mjs`
  for `hd:rt` (`block`, `abort`, `stderr`), `hd:prim` (`prim.rs` ports
  `float.mjs`: Burger and Dybvig shortest text and exact fixed-point text
  over a small big integer; `parse_f64` checks the grammar and uses
  Rust's correctly rounded parse; case mapping is Rust's full Unicode
  mapping with the final sigma), `Console`, `ConsoleInput`, `Clock`
  (`now`, `monotonic`, `sleep`), `Args` and `Env` (grant check and the
  once-per-name notice). Each import is bound with `Linker::func_new` at
  the module's own import type, so a width difference (`usize` digits as
  `i32`) needs no second signature. The output contract is the Node
  host's: standard output is held until standard error is written or the
  run ends, a panic stub's report and status 3, an entry's own status
  (`.Err` is 1, `ExitCode(n)` is n), `internal error: <V8's trap text>`
  and status 3 for any other trap, the deadlock report, and
  `panic: host-contract: ...`. Two traps Node cannot report are spec
  panics here: a stack overflow is `stack-exhausted` (Node ends with V8's
  `RangeError` and status 1) and a refused exchange-buffer growth is
  `heap-exhausted`.
- **Waiting (`reactor.rs`).** No host call blocks on an operation that is
  intrinsically asynchronous: `sleep.start` and `write_line.start` return
  a handle (status 1), and `read_line.start` is pending too while a
  reader thread of its own reads the next line of standard input. The
  driver loop and `hd:rt` `block` wait on one condition variable for the
  earlier of the next timer deadline and a delivered line. A read that
  is aborted loses no input: a line read for it goes to the next read.
  Console writes complete at once and reach the stream when the held
  output is written, as on Node. Http and Net deliver to the same
  reactor from their client threads (§20.4); no async runtime is linked.
- **Not yet on wasmtime at W1.** `FsRead`, `FsWrite`, `Process`, `Random`,
  `Sys`, `Http`, `Net`, the handle traits, `TestRunner` and
  `PropertyRunner`, and the conformance runtime profiles. A module that
  imports one is refused at load: ``the program uses `FsRead` (`hd:FsRead`
  `read_text.start`), which is not yet on wasmtime; run it on Node,
  without `--engine wasmtime` `` (status 101). `hd test` stays on Node.
- **Tests.** `hd_cli/tests/wasmtime_engine.rs` runs the samples and
  programs for arithmetic, panics, `.Err` from `main`, `ExitCode`, `Args`,
  `Env` under grants, float text, a suspending entry with `sleep!`, and
  console input on both engines and asserts the same standard output,
  standard error and status. `hd_driver/tests/float_text.rs` checks
  `prim.rs` against `float.mjs` on its 38,000 samples.
  `HD_CONFORMANCE_ENGINE=wasmtime` runs the driver conformance suite with
  every runtime entry program whose imports all run on wasmtime on both
  engines, judged by its wasmtime run: 129 programs, 128 identical to
  Node; the one difference is `runtime/panic/unbounded-recursion.hd`,
  which passes on wasmtime (`stack-exhausted`) and fails on Node. Five
  runtime-profile programs stay on Node.
- **Measured** (aarch64 macOS, load about 10, median of 5 runs of `hd
  FILE.wasm`, prebuilt debug modules, wall ms, Node v24.19.0):

  | Program | Node | wasmtime | wasmtime `--release` |
  | --- | ---: | ---: | ---: |
  | hello (startup) | 41 | 7 | 8 |
  | closures | 374 | 1337 | 1186 |
  | int-loop | 2397 | 1054 | 1372 |
  | list-sort | 1623 | 1293 | 546 |
  | map-count | 48 | 14 | 13 |
  | string-build | 2611 | 166 | 167 |
  | trait-dispatch | 895 | 811 | 711 |

  Startup is about 6x faster: no Node process, and Cranelift compiles a
  5 KB module in a few milliseconds. `closures` (20 million `call_ref`
  closure calls over a list) is 3.6x slower than V8, which inlines the
  closures; a 1 GiB initial GC heap did not change it, so it is code, not
  collection. The `hd` binary grows from 7.90 MB to 19.02 MB (6.57 MB to
  14.68 MB stripped). The hello module is unchanged (5,209 bytes).
- **Left after W1.** The other host traits (files under `cap-std`, process,
  random, sys, then Http and Net on client threads, as §20 plans), `hd test` on
  wasmtime (a `Linker` per host kind, `InstancePre`, the pooling
  allocator, test timeouts through epochs), the `cwasm` and incremental
  compile caches of §18.2, `--max-heap` and `--time-limit` through a
  `ResourceLimiter` and epoch deadlines, the conformance runtime profiles
  as Rust providers, making wasmtime the default, and then dropping
  Node.

### 18.8 As Built (W2, #244): Core Host Providers

W2 adds `FsRead`, `FsWrite`, `Process`, `Random` and `Sys` to
`hd_run_wasmtime`. Node remains the default engine.
The import signatures, exchange buffer and generated program code stay
unchanged.

| Provider | Implementation and grant checks |
| --- | --- |
| `FsRead`, `FsWrite` | Synchronous `std::fs` calls; `Grants::covers_path` resolves symlinks and existing ancestors before access. Rename checks both paths. |
| `Process` | `std::process::Command`; inherited environment and instance-local working directory, with `Grants::covers_name` before spawning. Pipes are drained concurrently while input is written. |
| `Random` | `getrandom 0.4` reads operating-system randomness; total denial remains the CLI's startup check. |
| `Sys` | `std` platform, architecture and CPU reads, plus fallible `hostname 0.4`; each method checks `Grants::covers_name`. |

File and process operations return status zero and an encoded result
inside `.start`, as on Node. Their `.finish` imports report a stale
operation as `host-contract`.
The decoder copies arguments before results overwrite the exchange
buffer, and rejects truncated encodings.

Filesystem results preserve byte-sorted listings, symlink metadata,
strict UTF-8 text decoding and the initial BOM behavior.
`libc` supplies errno constants for Node's error spelling; the host
contains no unsafe code.
Process captures both streams as lossy UTF-8, preserves exit status,
and maps termination by signal to 128.
Unix pipe workers cancel together at the aggregate output cap, including
pipes retained by descendants. Non-Unix blocking pipes lack that cancellation;
the limitation is recorded in Diagnostic Notes.
W2 originally used safe POSIX spawn wrappers to preserve Node's executable-format errors.
The owner subsequently rejected Node behavior as the authority for W4.
`Command` now supplies spawning and child cwd without additional dependencies or unsafe code.
Its platform behavior can invoke a shell for executable text without a shebang on macOS.

The owner approved the corresponding Node host correction for invalid
process arguments. Both engines return `NotFound` for an empty program
and `Other` for NUL-containing names or arguments, after grant checks.

The current ABI declares handle traits only for Net.
Those move with Http and Net in W3; W2 introduces no new handle API.
The runner providers and conformance runtime profiles remain on Node.

`hd_run_wasmtime/tests/core_parity.rs` runs 23 native ABI parity tests
on both engines, comparing stdout, stderr, status and encoded results.
They cover every core method, scoped grants, filesystem failures,
process pipe pressure, malformed encodings and stale operation handles.
Random tests compare lengths and entropy properties instead of random bytes.

Native validation runs on macOS. The full runtime library also passes
a Windows cross-target `cargo check`; Windows execution remains untested.
Linux's full cross-check needs the unavailable `aarch64-linux-gnu-gcc`.
An isolated check compiles the exact core and codec sources against
Linux dependencies and Wasmtime types; it is not a full engine check.
Static review found Node's Windows path-prefix check and libuv's nested
Linux CPU-quota calculation can diverge from Rust's implementations.
These platform-specific issues are recorded in
[Diagnostic Notes](../../audit/compiler/diagnostic-notes.md), not claimed as verified parity.

The 12 CLI engine tests pass, including startup denial for every core
provider. Driver conformance before and after W2 runs 129 runtime
programs on both engines: 128 match exactly, and the documented
unbounded-recursion case differs.
Five runtime-profile programs remain on Node; the profile providers
belong to the test-runner port.

The hello sample is unchanged at 5,210 bytes in both fresh caches,
with SHA-256 `abe5f22751445776579828facd8bce6d7ae7d29dd2cf56f8bad906556f883c1a`.
W1 reported 5,209 bytes; these current before and after modules are
byte-identical.

### 18.9 As Built (W3, #245): Http And Net Providers

W3 adds `Http`, `Net`, `TcpStream`, `TcpListener` and `UdpSocket`
to the wasmtime default profile. Node remains the default engine.
The import signatures and generated program code remain unchanged.

Http uses one lazily started shared client thread and synchronous channel
replies. Each redirect rechecks the target's host and port before DNS or I/O.
The ten-redirect limit, method rewrites and whole-chain deadline follow
the Node provider.

Pinned `ureq 3.4.2` supplies TCP, TLS and response-body framing.
A plaintext transport adapter preserves raw header spelling, ordering,
repeats and Latin-1 values on both TCP and TLS connections.
It writes the original request headers and body, preserving explicit
framing and the URL-derived or caller-supplied `Host` header.
Proxies, connection reuse and automatic decompression are disabled.

The resolver hands every address to the connector through hop-local
state, bypassing ureq's bounded resolver result.
One absolute deadline covers DNS, candidate connections, TLS and the
whole response body, including time spent waiting for the shared worker.

Net uses a separate lazy worker for each host instance.
That worker owns safe nonblocking `std::net` sockets configured through
`socket2`, with queued arrivals and retained EOF or error states.
Handles retain their closed state, and each UDP destination receives its
own grant check.
The owner-approved M19 Q1 follow-up adds a metadata-only Node preflight.
Closed UDP handles return `Disposed` before destination validation;
unknown handles remain `host-contract`, and every live send checks its grant.

Hostname connections alternate deduplicated resolver families, preferring
the first returned family.
Nonfinal attempts receive 250 milliseconds, matching
[Node's family-selection timeout](https://nodejs.org/download/release/v24.21.0/docs/api/net.html#netgetdefaultautoselectfamilyattempttimeout).
The final or sole attempt waits for completion or host cancellation.

Every network operation completes inside `.start`, returning status zero
and an encoded result, as on Node.
The `.finish` imports reject stale operations as `host-contract`.
Ending a run stops its Net worker and releases its shared Http worker
guard, even when an embedding retains the ended instance.

The network suites contain 18 Http and 12 Net tests, using local servers
only. Twenty-seven compare both engines; three separately check decided
spec behavior or retained-instance cleanup.

M21b removes post-stop port-rebinding assertions from network cleanup tests.
Unit tests observe the socket owner's disconnected request channel after teardown.
Retained-instance tests observe peer EOF while the ended instance remains alive.
Refusal fixtures retain established connections whose source ports reject new connections.

The listener-accept parity fixture still needs a temporary port handoff:
the Net ABI exposes no listener-address query for a guest that binds port zero.

Certificate-failure tests compare the error category, not provider-specific
TLS messages. The original 23 core parity tests remain unchanged.
Twenty-three Http and nine Net unit tests cover internal protocol and lifecycle
edges. Native validation runs on macOS.

The original M19 Windows cross-check stopped while building `ring`.
M21's pinned Zig-backed cross-target checks passed; native Windows execution
remains unverified.
DNS cancellation and observed Node/spec discrepancies are recorded in
[Diagnostic Notes](../../audit/compiler/diagnostic-notes.md).

Driver conformance on the current checker runs 131 runtime programs on
both engines: 130 match exactly, and the documented unbounded-recursion
case differs. Five runtime-profile programs remain on Node.
The prior W2 run's 129 programs and 128 matches are recorded above.

Fresh Node and wasmtime caches still produce the identical 5,210-byte
hello module, with SHA-256
`abe5f22751445776579828facd8bce6d7ae7d29dd2cf56f8bad906556f883c1a`.
The macOS release binary is 22,244,944 bytes, including W3's dependencies.

### 18.10 Resource Limits (M21, #269): As Built

M21 implements §22 for standalone source files, prebuilt Wasm and package runs.
The owner deferred per-case runtime limits until W4 supplies the wasmtime runner.
The owner also approved driver stage observation and CLI compiler-memory adapters.
The required native push gates passed on 2026-10-10.

`--time-limit` uses a shared 10-millisecond epoch ticker and an absolute
per-store wall deadline. Actual wall-clock checks prevent an early ticker wake
from expiring a fresh store. Initialization, polling and reactor waits share
that deadline; expiry reports `panic: time-limit` with status 3.

Http uses the earlier request or program deadline across queueing and each hop.
An earlier request expiry remains an encoded Http timeout; program expiry panics.
Net socket loops and DNS waits check the same program deadline.
Uncancellable OS resolver helpers may finish later, but cannot open sockets.

Unix Process expiry cancels nonblocking pipe workers, kills the direct child
with SIGKILL when necessary, and reaps it. Unix filesystem pipe operations use
nonblocking descriptors and deadline-aware polling. Native calls check deadlines
between chunks and filesystem steps; mutations are never delegated to detached workers.

Epochs cannot interrupt a single blocked native kernel call.
Examples include filesystem access, output writes and entropy acquisition.
Windows Process uses anonymous stdin and private named stdout/stderr byte pipes,
with synchronous child handles and nonblocking parent handles.
Anonymous stdin retains queued input after the parent writer closes normally.
Filesystem named pipes use the same synchronous
polling backend; `WouldBlock` retries check the remaining deadline.

Pinned `interprocess 2.4.4` configures pipe wait mode through safe owned-handle
conversions. The handles then use safe `FileExt` calls, preserving the distinction
between an idle pipe and EOF. Consuming conversions avoid deferred flush workers;
no pending I/O operation or userspace buffer survives a completed polling call.
Anonymous stdin uses a private configure-only listener adapter, never an accept
or creation operation. The pinned source calls only the documented mode-setting
function, then returns the owned handle without flushing.
Microsoft documents anonymous handles for
[mode setting](https://learn.microsoft.com/en-us/windows/win32/api/namedpipeapi/nf-namedpipeapi-setnamedpipehandlestate)
and buffered stdin delivery after
[writer close](https://learn.microsoft.com/en-us/windows/win32/procthread/creating-a-child-process-with-redirected-input-and-output).

Safe `winapi-util` handle classification catches named pipes reached through aliases.
Pipe mode changes may require extra attribute rights; OS permission errors are
returned rather than falling back to blocking I/O. Remote SMB pipe opens remain
subject to the single-kernel-call limitation.

Darwin poll/kqueue does not wake for an empty FIFO writer disconnect.
Safe select and low-descriptor duplication fix ordinary descriptor cases.
When all descriptors below 1024 are occupied, an empty writer disconnect may
instead time out; data remains readable. The owner accepted documenting this
rare edge rather than adding an unsafe dynamic-select wrapper.

`--max-heap` installs `ResourceLimiter` before instantiation.
Wasmtime 49.0.2 sends both linear and GC backing allocations through that limiter;
no pooling or GC sampler is necessary. Initial backing bytes, growth and both
copying semispaces share one cap, including page rounding and geometric growth.

The backing-byte cap excludes reserved virtual address space, native tables and
host/runtime metadata. Bounded runs initialize GC capacity lazily, rather than
reserving the profile's 64 MiB immediately. A recovered GC expansion refusal
does not panic; only a propagated allocation failure reports `heap-exhausted`.

`--max-memory` overrides `HD_MAX_MEMORY`; omission leaves compiler enforcement off.
The safe counting allocator tracks live wrapped Rust allocation bytes, not RSS.
System allocation failure can abort before its callback; mapped memory, native
allocations and stacks are outside this approximate compiler cap.

Driver stage scopes identify allocations on each compiler worker.
A precompilation fallback labels hidden Cranelift workers without overriding
an explicitly observed stage. One prepared reporting thread writes the preallocated
`memory-limit` diagnostic and exits 101, avoiding allocator-time stdio reentry.

Compiler accounting continues during guest execution, but enforcement ends at
guest entry. This avoids charging detached guest-worker or runtime-cache allocations
after teardown; the command performs no subsequent compilation.
Cranelift compilation remains under the compiler cap.
Neither program limit has a default; the embedding API still honors supplied limits.
Until W5, Node warns and ignores both explicit runtime limits, as the owner directed.

Focused tests cover flag forms and routing, allocator-stage errors, combined heap
growth, busy Wasm loops, reactor waits, native sockets and Unix process/pipe cleanup.
Both full release workspace passes succeeded, first with conformance-report updates,
then against the recorded passing-case baseline. The report required no changes.
The runtime's 88 unit tests and nine bounded deadline regressions passed.
The CLI's 28 binary unit tests and seven limit-routing regressions passed.

Release formatting, workspace all-target Clippy, spec checks and website build passed.
Windows CLI/runtime all-target Clippy passed with the pinned Rust 1.97.1 toolchain
and `x86_64-pc-windows-gnu`, including seven polling-backend unit tests in compilation.
Windows-native execution remains unverified; cross-compilation is not runtime evidence.

### 18.11 Conformance Runtime Tiers (M22a, W4 Part 1)

`HD_CONFORMANCE_ENGINE=wasmtime-only` runs built entry programs in-process.
The driver never silently falls back to Node when Wasmtime refuses a program.
`wasmtime` compares both engines, including the fixture runtime profiles.
Unset, empty or `node` keeps the existing Node default.

Fixture providers are explicitly scoped to a selected engine.
The small `profiles.rs` module uses the shared exchange-buffer codec.
Gauge returns 300, Sensor returns NaN, infinity and negative zero,
and Vault returns its input length without changing the boundary bytes.

Sensor state resets for each instance; the default host rejects these imports.

`pending-write` uses the existing pending Console provider.
The other profile directives absent from Node's `profiles.mjs` are not invented here.
Their existing compiler/spec failures remain visible in conformance results.
Nine raw-Wasm provider tests and the engine-selection tests pass on macOS.

TestRunner and PropertyRunner fixtures remain on Node under M22a's explicit deferral.
The driver reports each remaining path and invocation count.
`HD_CONFORMANCE_TIER=lang` or `std` selects runtime cases for isolated timing.
Neither non-default engine modes nor tier-filtered measurements rewrite `CONFORMANCE.md`.

The following single samples use eight workers on the shared macOS machine, from main commit `00018201` plus M22a.
Times include selected-suite compilation and execution, but exclude Cargo compilation.
They are not stable benchmarks: external load varied, and most fixtures still use Node.
The mode does not yet establish an overall speedup.

| Tier | Indexed cases | Wasmtime entry programs | Remaining Node test cases | Node wall | Wasmtime-only wall |
| --- | ---: | ---: | ---: | ---: | ---: |
| language | 794 | 130 | 595 | 11.512 s | 8.618 s |
| stdlib | 240 | 7 | 230 | 4.716 s | 4.453 s |
| CLI | 102 | 44 attempted program commands | 24 | 4.501 s | 5.606 s |

Of 137 entry programs compared, 136 match standard output, standard error and status exactly.
Language results are 733 pass, 22 fail and 39 unsupported on Wasmtime.
Stdlib results are 235 pass, three fail and two unsupported on both engines.
The only entry-program difference is the previously observed Node stack-overflow bug.

Node 24.19.0 reports an uncaught `RangeError` and status 1 for `unbounded-recursion.hd`.
Wasmtime reports the required `stack-exhausted` panic and status 3.
Consequently, Node has one fewer passing language case; frozen fixtures remain unchanged.

The CLI mode selects Wasmtime only for program-command routes.
It preserves explicit engine flags, non-runtime commands and arguments after `--`.
Both measured modes have 87 passing and 15 failing CLI cases.
The selected run attempts 44 Wasmtime program commands and 38 Node test commands.

Attempted commands are not guest-execution counts: compilation or usage can fail first.
Twenty-four CLI case paths still invoke `hd test`; none selects a Node entry program.
Exact remaining paths are printed by the runners with `--nocapture`.
M22 closes these test-runner dependencies before M23 changes the default engine.

CLI comparison runs each complete fixture twice, resetting fixture and cache at identical paths between engines.
It compares original output bytes, native exit status and timeout state for program-command steps only.
All 44 observed program-command pairs match, with no differences, missing pairs or explicit-engine exclusions in this suite.
The comparison sample takes 9.571 seconds; ordinary fixture judging retains its existing text policy.

Validation found the existing FIFO test joining a peer blocked in `open`, after the runtime write had returned successfully.
Bounded peers then exposed one empty-read assertion failure during a full gate; ten subsequent 89-test runtime-suite runs passed.
Isolated probes passed 400 Node transfers and 1,600 Rust transfers, including FIFO reuse and overlapping open-return publication.
No production defect or definitive cause was established.

The success fixture now confirms reader-open readiness before append, retaining exact bytes and EOF assertions.
Its writer remains delayed; the separate no-peer write-open timeout test retains deadline-retry coverage.
All peer I/O and completion waits are bounded; the append success assertion covers a connected reader, not delayed-reader arrival.
Production filesystem behavior and the documented high-descriptor limitation are unchanged.

### 18.12 Host Quality Review (M24, 2026-10-11)

Status: source review; fixes below remain pending until verified and pushed.
This review changes no language or library rules.
It covers commits `3707b290`, `43993c72`, `91337f6f`, `0f319ffc`,
`0613e39f`, `3459768f`, `ec517898`, and W4's current integration.
The feedback is in [Muse's queue](../../audit/muse_task.md).

| Feedback | Assessment and evidence | Action |
| --- | --- | --- |
| Size and shape | Agree. Large commits obscured review boundaries. File totals include substantial tests; HTTP transport/parsing and provider policy remain distinct production concerns. A shared primitive codec already exists, but domain variants are handwritten. | Use small concern-specific commits. Remove compatibility tables before moving files. Validate import signatures before effects; keep genuine ABI schema generation outside this grant. |
| Node parity versus spec | Agree. Fs, Process and Net emulate Node messages without a spec requirement. `read_text` also strips an initial BOM, contradicting content preservation in `std-fs.read.text`. | Use native contextual errors while preserving specified error variants, grants and deadline behavior. Preserve the BOM. Record cross-engine differences rather than reproducing Node. |
| Grants | Agree that extensions need explicit approval. The owner approved the stage observer and CLI adapters in this thread; W4 separately received compiler-root and lockfile approvals. That approval does not grant unrelated edits. | Keep each fix within M24's named files; ask before ABI, compiler or CLI adapter expansion. |
| Global allocator cost | Agree. `startup` installs tracking and starts a reporter on every invocation. The dependency also wraps allocations even when tracking is disabled. | Enable callbacks and the reporter only when a compiler cap is requested. Keep balanced accounting once enabled; do not claim zero wrapper overhead. |
| Test robustness | Agree. The port-rebind race was fixed, but some HTTP accepts remain unbounded. Process PID-file assertions start their deadline before fixture readiness. | Use bounded accepts and explicit readiness. Test Process worker-creation failures with deterministic injection, not longer sleeps. |
| Owner's quality assessment | Agree with the underlying criticism. Exact parity accumulated avoidable policy and hid native failure paths. | Prioritize removals, explicit ownership and failure cleanup over cosmetic file moves. Remove unused dependency features; retain libraries only for demonstrated capabilities. |
| Replay readiness | Agree with the required boundaries, not a claim that replay is implemented. `wake(handles)` ignores handle values; completion wrote them earlier. Import replies mix scalar returns with direct memory writes. Abort and termination lack named central controls. | Make wake delivery consume its batch, centralize encoded import replies and existing abort/stop cleanup. Add no recorder, cancellation export or journal feature. |

The replay check follows [Live Execution §§5–6](live-execution.md#5-replay).
HTTP and Net timeout outcomes already return through dispatch.
Their private clock reads need not become separate journal events.
Busy epoch deadlines are nonreplayable stops, not deterministic root cancellation,
as [§7.3](live-execution.md#73-stops-ctrl-c-while-running) distinguishes.

Rust's std filesystem, process and socket APIs remain the first choice.
The engine, TLS/HTTP, entropy, hostname and JSON dependencies supply substantive functionality.
Safe descriptor polling and Windows pipe adapters provide cancellation that std lacks.
Typed TLS and initial engine-allocation errors justify the direct rustls/rustix dependencies.
The currently used socket2 APIs need no `all` feature.
`hd_host_abi` has codec names, not structural schemas; moving codec.rs there would not eliminate handwritten domain layouts.

Fix order: conditional allocator setup; native Fs/Process/Net errors and BOM preservation;
fallible Process workers and bounded fixtures; explicit reply/wake/control boundaries.
Each code fix gets focused dev regressions and lint checks.
The owner approved full gates once on the completed M24 tree, before its push, on 2026-10-11.
Owner direction now forbids `--release`, including gates and lint checks.
Native Windows/Linux execution and the owner-deferred workspace compilation split remain unverified or deferred, not fixed by this review.

The allocator fix conditionally enables the tracker and reporter.
It recognizes global compiler caps and `HD_MAX_MEMORY`, but not guest arguments after `--`.
Thirteen focused dev-profile allocator tests pass, including disabled tracking,
pre-existing untracked frees, stage diagnostics and retained guest allocations.
Full dev-profile gate results remain pending.
The first full dev run reproduced both Process fixture startup races.
Their deadlines preceded child readiness, allowing expiry before PID-file creation.
The runtime unit suite passed 123 tests and failed those two setup assertions.
Driver conformance passed; the observed failures do not establish a production timeout defect.
The Process fix uses fallible pipe-worker creation and cleanup before scoped joins.
Both deadline fixtures prove readiness before arming their unchanged 250ms budget.
Eleven focused Unix dev-profile tests pass, including all three injected worker-start failures.
Focused tests and workspace lint checks pass for this separate runtime fix.
Full gates remain pending on the completed M24 tree.

The native-error fix removes filesystem, Process and Net errno-message emulation.
Rust's native error text carries operation and path/address context; spec-defined variants remain stable.
Three Windows access-refusal codes retain `PermissionDenied` classification when std leaves them unclassified.
UTF-8 BOMs remain file content, and a started child's stdin `BrokenPipe` preserves output and status.
Net retains native UDP-send failures and informative candidate errors, without Node preflight messages or empty aggregates.
Read EOF no longer closes TCP's write half: only explicit close disposes the handle.
TCP fixtures accumulate partial reads under a bound rather than assuming message-sized reads.
Native-only regressions cover these intentional Node differences; shared-shape differential tests remain.
Focused dev verification passes 127 runtime units, 25 core-provider tests and 12 socket tests.
The unused `socket2` all-features flag is removed; no dependency or unsafe code is added.
Other post-start Process I/O failures and output-cap failures still return `Other`; their result policy remains a follow-up.

## 19. Test Runner

### 19.1 Building

1. `hd check --tests` (§7.3), then the test plan: **one unit-test
   program per package**, holding every module's `tests:` cases and doc
   tests, and one program per integration test file (systems review,
   finding 2; the owner accepted the review's recommendation).
   - **Init per module.** The package program exports one init function
     per module with tests (codegen.md §13.10). A case's instance calls
     the init export of its own module, which runs exactly the init
     groups that module reaches, in D1's order. That is the set a
     per-module program initialized before, so every case still runs in
     a fresh instance after its module's initialization
     ([`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance)).
   - **Why one program.** Programs share code entries, but each program
     collects, links, Cranelift-compiles and stores its own copy of the
     shared code: std's collections, formatting, `Debug` and the
     `std.testing` harness. With one program, shared code is collected,
     linked and compiled once.
   - **Integration tests stay one program per file.** Their environments
     differ ([Test Environments](../../spec/cli/command-line.md#test-environments)),
     and each sees only the package's public interfaces.
   - **A build error in one module's tests** (such as
     `instantiation-too-deep`) is reported for that module's cases.
     `Collect` drops the roots whose walk reached the error, and the
     other modules' cases are built and run.
2. The package's executables are built first
   ([`cli.test.builds-executables`](../../spec/cli/command-line.md#r-cli.test.builds-executables)),
   in the test profile ([`cli.profile.test`](../../spec/cli/command-line.md#r-cli.profile.test)).
3. Each program goes through `Collect`, `Emit`, `Link` and `Precompile`
   as tasks, in parallel across programs. Warm, each program is one
   `prog_key` hit and one `cwasm` hit.
4. **`--filter` builds only the selected cases** (lowering pass). The
   filter is matched against the test plan's registration names, which
   are string literals known after checking, before collection. The
   program's roots are then the selected `TestCase` bodies plus the
   module's reachable init groups, and the selection joins the root
   description, so a filtered program has its own `prog_key` and shares
   every code entry with the full one. Its roots are the selected
   `TestCase` bodies and the init exports of their modules only, so
   `hd test --filter one` builds about what one module's program built
   before (about 800 instances at 10k lines), not the package's. That
   halves the functions to compile and look up for `test-latency`.
   `--affected` selects whole modules' cases the same way (commands.md
   §7.4). A build error that only an
   unselected case reaches, such as `instantiation-too-deep`, does not
   appear under that filter; a plain `hd test` still reports it, and a
   failure's repro command already uses `--filter`, so the case it names
   reproduces.

**Cost of the test plan** (`tests-1k`: 1,000 unit tests in 100 modules;
compile-study sizes of about 800 instances and 250 KB of code per
module's program, of which most is shared; the package program is
estimated at 5,000 to 6,000 instances and 1.5 to 2 MB; the review's Mac
I/O costs):

| Cost | 100 module programs (before) | One package program |
| --- | --- | --- |
| collect, at about 2 ms per 800 instances | 0.2 CPU-s | about 15 ms |
| link reads | 80,000 code entries, or 100 × 30 packs | about 30 packs: 2 to 4 ms |
| Cranelift, cold, at 1.5 µs per byte | 10 CPU-s with the per-function cache shared, 38 if not | 2 to 3 CPU-s |
| Cranelift lookup pass after an edit to a widely used module | 100 programs × 12 to 90 ms | one program, about 0.1 to 0.6 CPU-s |
| `cwasm` on disk | 100 to 200 MB per package state | 5 to 10 MB |
| `cwasm` loads per warm `hd test` | 100 | 1 |
| constant globals built per case instantiation | one module's, about 1,500 | the package's, an estimated 2,000 to 3,000: about 100 to 150 µs per case |

The last row is the price: every instantiation builds every immutable
constant global of the program, so a case pays for constants its module
never reads. At 1,000 cases that is about 0.1 to 0.15 CPU-s, inside the
1 ms per test budget. Slice 8 measures it; a large package can split its
program per folder with no other change.

### 19.2 Listing Cases

- Registration names are string literals, so the linker writes each
  program's cases into `hd.runtime` (§16.4): export index, name, kind
  (`it`, `it_each`, `it_prop`, doc test), the module's init export, and
  the registration's anchor, resolved to a file and line when printed
  (codegen.md §13.8, "Positions").
- `--filter` selects cases from the test plan before anything is built
  (§19.1), and the built program lists only those cases.
- **`it_each`** rows are evaluated at run time
  ([`std-testing.it-each.rows-at-run`](../../spec/std/testing.md#r-std-testing.it-each.rows-at-run)).
  The first instance of an `it_each` case runs row 0; its `row(count)`
  call tells the runner the count. The runner then schedules rows 1 to
  `count - 1`, each in a fresh instance. A filter that names `name[i]`
  still runs row 0 first to learn the count, and reports only the rows it
  names.

### 19.3 Running

```text
work queue: (program, case) in content order: program path, then registration order, then row
workers (--jobs): pull a case, then
    store = Store::new(engine, limits)              ;; pooling slot
    inst  = instance_pre.instantiate(store)         ;; imports pre-resolved
    inst.init(case.module)                          ;; the case's module's init export: the groups it reaches
    outcome = drive(inst.call_test(case))           ;; poll/wake loop; reactor only for integration tests
    drop(store)                                     ;; slot returned
results: slot per case; printed by the release cursor in content order (§6.5)
```

- **A fresh instance per case**
  ([`module.testing.instance`](../../spec/lang/10-modules.md#r-module.testing.instance)),
  made cheap by `InstancePre` and the pooling allocator. Starting from a
  snapshot was dropped by the owner, so module initialization runs per
  case.
- **Host setups.** A unit test gets `TestRunner` alone. An
  integration test or doc test gets the default profile, `TestRunner`, the
  test runner's `Process`, the test grant, the package directory as its
  working directory, no arguments and closed input
  ([Test Environments](../../spec/cli/command-line.md#test-environments)).
- **`temp_dir()`** makes the case's directory on first call and the
  runner removes it when the case ends
  ([`cli.test.env.temp-dir`](../../spec/cli/command-line.md#r-cli.test.env.temp-dir)).
  A unit test that never calls it costs nothing.

**Per-case overhead budget** (target: at most 1 ms per test, and 1,000
unit tests in at most 1 s warm):

| Step | Estimate |
| --- | --- |
| take a pooling slot and instantiate from `InstancePre` | 5 to 20 µs |
| `hd.init` of a small test program | 1 to 50 µs |
| call, poll, `TestRunner` calls | 1 to 5 µs |
| report into the result slot | under 1 µs |
| total per case | about 0.1 ms, before the test's own work (an estimate, not a bound) |

Warm `hd test` on 1,000 unit tests in 100 modules: process start and the
warm check fast path (about 30 ms), one mapped `cwasm` load for the
package's unit-test program (a few ms), and 1,000 cases over 8 workers. That is well under a
second **if** each case's init is small. A fresh instance repeats all
reachable initialization and constant-global allocation. At 2 ms of
init per case, 1,000 cases cost 2 CPU-s and at least 250 ms of wall
time on 8 workers. **Status: not established** (Codex review, P8). The
`unit-test-perf` benchmark fixes its module count, init work per module
and body work per test, and slice 8 measures store creation, constant
globals, `hd.init`, teardown and pool reset separately. It also counts
the executable builds that `hd test` needs first. `test-latency` (edit, then `hd test --filter one` in at most
300 ms) pays one module's check, the instances whose code keys changed,
one link, Cranelift for the changed functions, and one case.

### 19.4 Property Tests, Panics And Timeouts

**`it_prop`.** One property test is one test case, so all its generated
cases run in one instance (a first-release feature):

- The runner implements `PropertyRunner` on the host. `start` returns
  the case's `PropertyCase` through the exchange buffer; `record` is a
  scalar import, one per draw; `show` sends the input's `Debug` text.
- **A discard** is a panic before `show`
  ([`std-testing.runner.discard-read`](../../spec/std/testing.md#r-std-testing.runner.discard-read)).
  The instance is poisoned, so the runner takes a fresh instance and
  continues with the next seed. A discard costs one instantiation.
- **A failure** poisons the instance too. Shrinking runs on the host,
  over the recorded draws. Each shrink attempt replays in an instance;
  passing attempts reuse it, and a failing attempt costs a fresh one.
- **One property runs on one worker.** Its cases share one instance, so a
  case can see state that earlier cases left. Splitting the cases across
  workers would change that history and so the outcome (Codex review,
  D3). Workers run different properties in parallel instead. Within a
  property, the instance sequence is a function of the seed and the
  outcomes alone: a pass keeps the instance, a discard or failure takes a
  fresh one. So the result does not depend on `--jobs`. The
  `proptest-perf` target is per worker, so it does not need the split.
- **Regression file.** Saved streams replay first, and a new failure's
  shrunk stream is written to `__regressions__/`
  ([`std-testing.prop.regression-file`](../../spec/std/testing.md#r-std-testing.prop.regression-file)).
- **Throughput.** A simple generator's case is a few draws (each a
  scalar import of tens of nanoseconds), one `Debug` render and one short
  string crossing, and the property body. That is a few microseconds per
  case, inside the 10 µs that 100k cases per second allow, on one worker.

**`expect_panic`.** The panic's category is read from the instance's
globals (§15.5). A matching category passes the case; completion or
another category fails it.

**`timeout`.** `report_timeout(millis)` sets the store's epoch deadline
and a reactor timer, so a busy loop traps and a pending host wait is cut
at the same moment. Either fails the case with `time-limit`. A case
without a `timeout` has no deadline unless `--time-limit` sets one.

### 19.5 Reports

- **Quiet passes.** Passing cases print nothing; a summary line ends the
  run. JSON lines list every case.
- **Each failure** prints its name, the panic category and message, the
  symbolized backtrace (§15.5), and a **repro command**:
  `hd test src/billing.hd --filter "sums prices"`. A property failure adds
  its seed and the regression file path; rerunning the repro replays the
  saved stream.
- **Structured assert diffs.** `assert_equal` in `std.testing` compares
  the two values through their derived structure and builds a message
  that lists only the differing paths (`.items[2].price: 3 != 4`). It is
  std hd, so the runner only prints the message.
- **Order.** Results are printed in content order by the release cursor,
  whatever order workers finish in.

### 19.6 Tests In The Browser

The playground runs a program's tests in its program worker with the same
Wasm. The JS glue implements `TestRunner` and `PropertyRunner`, and each
case gets a fresh `WebAssembly.Instance` of the compiled module.
Instantiation in a worker is synchronous for any module size.

### 19.7 W4 Implementation Status (M22, 2026-10-10)

Status: implemented, with the owner-approved deferrals below.
All required gates passed on `c3f965e8` plus the M22 implementation.

The owner approved connecting per-test `--max-heap` and `--time-limit`
in W4, as deferred from M21. Node remains the default in M22.
Explicit Node runtime limits warn and remain unenforced until W5.

The owner deferred structured panic reports because their compiler
prerequisites are missing. The current emitter exports functions and
`hd.x`, but no panic category or message globals.
It writes no `hd.sites`, `hd.lines` or `hd.names` tables.
W4 therefore retains existing panic output; §21.5 awaits compiler work.
This deferral does not establish symbolized-frame coverage.

The Node property runner creates a fresh instance for every body run.
The recorded §19.4 design instead retains the instance after a pass.
W4 follows the recorded design: discard or failure replaces the instance.
Differential results must distinguish this history-sensitive behavior
from ordinary output or codec regressions.

The bang `EntryPoll` previously retained its completed suspension root.
A second body run therefore panicked `suspension-invalid-state`.
The owner first deferred this prerequisite, then approved the compiler fix.
The wrapper now clears the root only after a ready result.
Pending polls retain their root; other instance state remains intact.
Directly polling a completed or cancelled suspension still panics.

The frozen scalar-generator and assume/discard fixtures reproduced the
problem in all seven property registrations; Node passed them.
Both ordinary registrations in `tests-block-items` matched.
After the fix, all seven property registrations and both ordinary cases
match Node. A broader selected comparison matches 15 test outcomes and
three program entries, with zero differences and four compiler-unsupported
fixtures. Runtime tests pass 116/116; emitter tests pass 5/5.
Two new emitter regressions cover void and reported completion, including
preservation of pending roots and report arguments.
Scoped all-targets clippy passes for `hd_wasm` and `hd_run_wasmtime`.
These early probes preceded the full conformance gates recorded below.

Full driver comparison on `c3f965e8` plus M22 edits preserved the checked-in
pass list. All 1,559 test outcomes matched, excluding elapsed-time fields.
Ordinary entries matched 140/141; the exception remains Node's logged
unbounded-recursion diagnostic, versus Wasmtime's `stack-exhausted` panic.

| Runtime tier | Test fixtures | Matching test outcomes | Matching ordinary entries | Node aggregate execution wall | Wasmtime aggregate execution wall |
| --- | --- | --- | --- | --- | --- |
| Language | 600 | 914/914 | 133/134 | 95.092 s | 19.538 s |
| Standard library | 230 | 645/645 | 7/7 | 38.511 s | 24.333 s |

The eight-worker suite took 32.270 seconds, including compiler work.
Aggregate execution time sums overlapping invocations, not suite elapsed
time. This is one local measurement, not an established performance bound.
Existing compiler failures and unsupported fixtures remain visible.

The CLI comparison preserved its pass list: 87/102 fixtures pass, with
15 existing failures and no unsupported fixtures. All 44 program-command
steps and 38 test-command steps match raw status and output bytes.
Neither missing pairs nor explicit-engine exclusions occurred.

CLI runtime-command wall totals were Node 28.302 seconds and Wasmtime
14.252 seconds; the eight-worker suite elapsed time was 9.242 seconds.
These totals include compilation and process startup, unlike the driver
execution measurements. Each engine received a fresh fixture and cache
at identical paths; setup-only commands are excluded from those totals.

Final formatting and workspace all-targets release clippy passed.
Both required full release workspace test passes succeeded after the final fixes.
The updating pass left `CONFORMANCE.md` unchanged; spec checks and the website build also passed.

Review found snapshot and regression file operations bypassed host deadlines.
Guest imports now reuse the filesystem provider's bounded pipe reads and writes.
FIFO regressions cover snapshot reads and regression loading.
Ordinary blocked kernel filesystem calls retain the OS limitation documented in §18.10.

A shrink replay that never starts cannot replace the original counterexample.
The shrink controller now stops without adopting that unavailable attempt.
A regression preserves its recorded draws, displayed input and original failure.

Regression persistence runs after guest execution, outside the completed body's deadline.
It writes a new regular file and renames it over the saved stream.
An expired body deadline therefore cannot erase the case report or abort later cases.
Regressions cover expired-deadline reporting, FIFO replacement and failed-rename cleanup.

The owner directed that Node behavior is subject to change, not an implementation authority.
W4 now uses `std::process::Command` for external children, including their case cwd.
Safe nonblocking pipe cancellation remains; no spawning dependency or unsafe wrapper was added.
Native regressions cover child cwd, literal arguments and the named-child external Process path.

The full updating gate exposed two HTTP fixture races, not a passing result.
Accepted sockets inherited nonblocking mode, so request reads could fail with `WouldBlock`.
Fixtures now explicitly select blocking reads with timeouts; a regression checks socket flags.
Production HTTP behavior is unchanged; all 125 runtime unit tests pass afterward.
Native heap regressions cover initial over-cap memory and independent growth limits across rows.

The owner also deferred the workspace compiler-memory-cap lifecycle fix
and requested logging it back to the orchestrator. Source review shows
that the current compile/run-per-member loop enters native guest execution
and permanently disarms the compiler cap before compiling later members.
Preparing all members before execution is a proposed remedy, not an
approved implementation: it changes user-facing `hd test` scheduling.
No split was applied, and no end-to-end workspace regression was run.
Normal `hd run` is outside this issue; this is separate from the completed
test-suspension root prerequisite above.

## 20. Porting The Node Host To Rust (M3, 2026-10-10)

The host today is `compiler/host/*.mjs` (core, float, http, net, test,
profiles, run; 2156 lines). It moves behind
`compiler/crates/hd_run_wasmtime` as wasmtime `Linker` definitions plus a
reactor, reusing two crates that already exist: `hd_host_abi` (the
capability tables, codecs, import names, handles) and `hd_run` (the
`Engine`/`Instance` traits, `Grants`, the test-runner model). The
wasmtime crate itself is not a dependency yet (owner approval pending,
SK-10); the version below is the input to that decision. Ground rules
for the port: host functions stay **synchronous** (no `async_support`,
no Tokio), the workspace lint `unsafe_code = "deny"` applies, and the
workspace today has zero third-party dependencies, so every new crate
below pays its way.

### 20.1 Provider by provider

| Provider / helper | Node API it uses | Rust replacement (version, size, why) |
| --- | --- | --- |
| `hd:rt` (`stderr`, `block`, `abort`) | `Atomics.wait` sleep loop, `process.stderr` | `std`: driver parks on `Condvar`/`mpsc::recv_timeout` (§20.3); stderr is `std::io::Write` to the sink |
| `hd:prim` (float format/parse, case) | pure ECMAScript (`float.mjs`, no `node:` import) | exact Rust port of `float.mjs` + `num-bigint 0.5` (S; replaces `BigInt` decimal scaling). `f64::log10` etc. are the same IEEE ops in the same order; any divergence is a conformance bug, caught by Q3 |
| `hd:Console`, `hd:ConsoleInput` | `Buffer`, stdout/stderr writes | `std::io` (`StdoutLock`/`StdinLock`); line buffering matches `out.push(line)` |
| `hd:Clock` (`now`, `monotonic`, `sleep`) | `Date.now`, `performance.now`, pending deadline | `std::time::{SystemTime, Instant}`; `sleep.start` is a pending op with `at = now + ms` (§20.3) |
| `hd:Random` | `crypto.randomBytes` | `getrandom 0.4` (XS, audited, no RNG state): `fill` reads bytes, `next_u64` reads 8 LE bytes |
| `hd:Sys` | `os.{arch,platform,hostname}`, `availableParallelism` | `std::env::consts::{ARCH, OS}`, `std::thread::available_parallelism`; W2 uses fallible `hostname 0.4` (XS), so failure returns `SysError.Unsupported` |
| `hd:Http` | `node:http/https` + a worker thread (§20.4) | `ureq 3.4` (M, blocking, rustls TLS, no runtime): redirect policy off with a manual ≤10-hop loop for per-hop grant rechecks and the 303/301/302+POST→GET rewrite; per-hop `timeout(remaining)`; bodies fully buffered (`Vec<u8>`), as today |
| `hd:Net` | `node:net`, `node:dgram`, `dns.promises`, a worker thread (§20.4) | `std::net` (`TcpListener`/`TcpStream`/`UdpSocket`) + `ToSocketAddrs` DNS on a per-instance worker thread; `Inbox` becomes `mpsc` channels with per-op reply senders |
| `hd:Args`, `hd:Env` | `process.argv`, `process.env` | `std::env::{args, vars}`; grant checks reuse `hd_run::Grants::covers_name`, path mapping reuses `resolve_path` |
| `hd:FsRead`, `hd:FsWrite` | sync `node:fs` (every call completes at once) | `std::fs`/`std::path` 1:1 (all calls are already synchronous, so they stay `Wait::Never`); `runProcess` becomes `std::process::Command` with piped stdio and the same 1 GiB cap |
| `hd:TestRunner`, `hd:PropertyRunner` | `node:fs` (regressions, snapshots, temp dirs) | `std::fs`; result JSON lines and `expect.txt`-style configs via `serde_json 1.0` (S; Q6); temp dirs via `std::env::temp_dir` + unique names (no crate) |
| `profiles.mjs` (conformance-only) | none | plain Rust closures over `put`/`Enc`, as today over `io` |
| `run.mjs` / `test.mjs` drivers | `WebAssembly.instantiate`, worker supervisor, `setTimeout` grace | `hd_cli` wiring over `hd_run_wasmtime`: one `Store` per case on the `--jobs` pool; supervisor timeouts via epoch deadlines (§20.3), no processes |

### 20.2 Exchange buffer and `hd_host_abi` codecs are reused

`hd_host_abi` is already the source of truth: the `Codec`/`Wait` tables
drive the import names (`imports_of`, `is_known_import`), and the
`Handle` type models the slot+generation scheme. The port adds the
Rust half that `generate_js_glue` is for the browser: `Enc`/`Dec`
operating on wasmtime `Memory` bytes (little-endian LEB128/zigzag/f64,
UTF-8 via `str`, same `HostContract` truncation errors), `put` writing
the framed result at offset 0 and growing with `Memory::grow`, and
completion words as LE `i32`s. The `Linker` definitions are generated
from the same tables, so a new capability touches one row, not N call
sites. `hd.wake`, `hd.init`/`hd.poll` keep their names and protocol
(§17.2, §19.3).

### 20.3 Suspension maps onto sync host calls plus epochs

The Node driver (`run` in core.mjs, `runOnce` in test.mjs) ports 1:1 to
a Rust reactor owning the `Store`: `init`, then `poll` until the status
is not `-1`, running due completions, calling `hd.wake(n)`, and parking
with the next deadline. `block_on` parks the calling thread
(`recv_timeout`), so wasmtime's `async_support` stays **off** and Tokio
is not a dependency: from the program's view every `.start` still
completes synchronously, exactly as the worker-thread blocking does
today. Timeouts use epoch interruption (`Config::epoch_interruption`,
already on in §18.1 and assumed by §19.4): a timer thread calls
`Engine::increment_epoch`, and `report_timeout`/`--time-limit` set
`Store::set_epoch_deadline` plus a reactor deadline, so a busy loop
traps and a parked host wait is cut at the same moment. `deadlock`,
`TimeLimit`→status 3, and the `stderr` panic lines keep their bytes, so
`hd test` output does not change.

### 20.4 Http/Net client threads and grants carry over

- **Http** keeps one lazily started client thread per process (as
  `httpClient` today): the program thread sends the request over `mpsc`
  and blocks on the reply. The manual redirect loop re-resolves and
  re-checks `Grants::covers_host` per hop (already unit-tested in
  `hd_run`), preserving the `*.suffix`/`:port` semantics.
- **Net** keeps one worker thread per host instance owning all sockets
  in a `handles` map, with `Inbox` ported to channels; `read`/`accept`/
  `receive` hold until data arrives, `close` marks disposed, async socket
  errors finish sticky errors. Grant checks stay on the program thread
  before posting (`connect`/`listen`/`bind_udp`/`send_to` re-checked per
  datagram, `lookup` by name only), reusing `covers_host`/`covers_name`.
- `stop` (today `worker.terminate`) becomes joining the threads on host
  `close`; in-flight blocking socket calls are woken by closing the
  socket first, as today.

### 20.5 Dependency budget

The `hd` release binary is 7.5 MiB today. Estimated additions (to be
measured at link time; the wasmtime row dominates and is the SK-10
decision):

| Crate | Version | Size | Why |
| --- | --- | --- | --- |
| `wasmtime` | 49.0 (stable; CLI spike used 49.0.2) | L (~100 transitive crates, Cranelift) | the engine; sync `Linker`/`Store`/epoch APIs cover §20.2-20.4 with no async runtime |
| `ureq` | 3.4.2, pinned | M (rustls, no Tokio) | blocking HTTP matching the sync model; manual redirects for grant rechecks; the raw-header adapter uses its unversioned transport API |
| `url` | 2 | M (IDNA and Unicode tables) | URL parsing and relative redirects, including authority and default-port grant checks |
| `socket2` | 0.6 | S | safe nonblocking TCP connect and listener/UDP setup, with platform socket options |
| `rustls` | 0.23, already transitive | no second TLS stack | identify certificate errors wrapped by the HTTP transport |
| `serde_json` | 1.0 | S | test-runner JSON lines and JSON configs (Q6) |
| `num-bigint` | 0.5 | S | `float.mjs` decimal scaling (Q3) |
| `getrandom` | 0.4 | XS | `hd:Random` without RNG state |
| `hostname` | 0.4 | XS | fallible `hd:Sys.hostname` (no std API) |
| `libc` | 0.2 | XS, already transitive | platform errno constants for Node-compatible filesystem and process errors; no unsafe calls |
| `nix` | 0.31.3 | S, Unix only | safe descriptor flags and readiness polling for bounded pipe I/O; signal helpers are test-only |
| `windows-sys` | 0.61 | already transitive, Windows only | filesystem flags and native error constants; no unsafe calls |
| `tracking-allocator` | 0.4.0, pinned | S | safe live Rust allocation callbacks for the approximate compiler cap |
| `rustix` | 1.1.5, pinned, already transitive | Unix only | identify concrete native allocation errors; no unsafe calls |
| `interprocess` | 2.4.4, pinned | S, Windows only | safe owned-pipe creation and wait-mode configuration; direct handle extraction avoids flush workers |
| `winapi-util` | 0.1.11, pinned, already transitive | Windows only | safe handle classification catches filesystem pipe aliases |

All remaining provider operations use `std`. Deliberately absent: Tokio (no async host
calls), reqwest/hyper (async runtimes or larger trees than `ureq` for
the same buffered semantics), `rand` (no distribution sampling; seeds
are discussed in Q4), TLS crates beyond what `ureq` pulls (Net is
plain TCP/UDP, as today).

### 20.6 Questions with a recommendation

- **Q1. Sync host calls only?** Yes (recommended): the Node host already
  presents synchronous `.start` semantics via worker-thread blocking;
  sync `Linker` functions plus a driver thread preserve them with no
  Tokio. Revisit only if a future capability needs true async I/O
  (e.g. streaming bodies).
- **Q2. `ureq` vs `reqwest::blocking` vs `hyper`?** `ureq` (recommended)
  for the smallest tree that fits the blocking model. Open
  implementation checks: `Host`-header override, raw header
  order/repeat preservation, and disabling connection reuse to match
  `agent:false`; if `ureq` cannot do any of them, fall back to
  `reqwest::blocking` (larger tree, Tokio inside, same sync surface).
- **Q3. Float bit-fidelity?** Port `float.mjs` op-for-op and add a
  differential conformance run (Node vs Rust over the std float corpus)
  before switching `hd test` to the Rust host; any divergence fails the
  gate.
- **Q4. Property-test PRNG?** `test.mjs` draws from seeded streams that
  replay from regression files. Recommended: hand-roll SplitMix64 in
  `hd_run` (30 lines, no dependency, identical streams on every
  engine) rather than adding `rand`/`rand_chacha`.
- **Q5. Net parity details?** Confirm at implementation: listen backlog
  (`std` 128 vs Node 511 default), IPv4/IPv6 dual-stack and
  `verbatim:true` DNS parity, and `read(max == 0)`/spillover edge
  semantics, each with a conformance probe.
- **Q6. `serde_json` or hand-rolled JSON?** `serde_json` (recommended):
  the test protocol is flat JSON lines and the team already audits
  small trees; hand-rolling saves kilobytes and risks divergence.
- **Q7. Pin wasmtime 49.0?** Yes (recommended): it is the newest stable
  that validates every module the emitter produces (M2 spike), pending
  the SK-10 linkage approval already recorded in `hd_run_wasmtime`.

Recommendation: approve the sync-only design with the §20.5 crate set
on wasmtime 49.0, and start with the reactor + `hd:rt`/`hd:Console`/
`hd:Clock` providers (no new dependencies), then Http/Net, then the
test-runner supervisor.

## 21. The Test Runner On wasmtime (W4, M9, 2026-10-10)

W1 (§18.7) runs `hd run` in-process; §19 fixes the runner model
(InstancePre, pooling slot per case, epoch timeouts, symbolized
reports). This section is the W4 build plan for `hd test` on wasmtime:
what gets built, in what order, and what each choice costs. No new
crates beyond §20.5 (`serde_json` for the JSON lines is already
budgeted). No code changes.

### 21.1 TestRunner and PropertyRunner providers

Port of the provider side of `compiler/host/test.mjs` (the supervisor
process model goes away; §21.5): `row(count)`, `report_timeout(millis)`,
`snapshot_check(len)`, `temp_dir()`, and `start/cases/shrink/examples`
plus `record(value)` / `show(len)`, as sync `Linker` functions over the
same exchange-buffer codecs as §20.2. `record` stays scalar (tens of
nanoseconds a draw, §19.4 throughput); `show` crosses one short string.
`temp_dir` makes the case directory on first call; the runner removes it
at case end (`cli.test.env.temp-dir`). Regression files and snapshot
comparisons are `std::fs` on the supervisor thread, not guest code.
`EndCase` (property exhaustion) becomes a Rust-side early return with
status 0, not an exception: wasmtime traps cannot resume, so the
provider records `ended` and the driver stops polling that case.

### 21.2 A `Linker` per host kind, one `InstancePre` each

Unit, integration/doc, and profile-bearing cases import different sets
(§19.3 host setups), and a `Linker` bakes its imports at
`instantiate_pre` time. So `hd_run_wasmtime` keeps three linkers —
unit (`TestRunner` alone), integration (`TestRunner` + default profile
+ test `Process` + test grants + cwd), doc/profile (plus the
conformance runtime profiles as Rust providers, closing the W1 gap) —
and one `InstancePre` per built test program per linker. A case takes
the program's `InstancePre` for its kind; a kind mismatch is a build
error, never a runtime trap. Host functions are shared across linkers
(one definition each, §20); only the import tables differ.

### 21.3 The pooling allocator

`hd test` uses the pooling allocator (§18.1): slots sized to the
`--jobs` worker count plus one spare per in-flight property shrink,
each slot with the test profile's small heaps (`unit-test-perf`,
§18.3), memories capped by `--max-memory` through a `ResourceLimiter`,
`max_wasm_stack` from the profile. Pooling makes instantiate +
teardown a slot checkout (the §19.3 5–20 µs budget) and bounds total
memory to slots × per-slot maxima — the property-shrink burst cannot
grow the process. One `Store` per case per worker, as §19.3 sketches;
stores never cross threads. Non-test `hd run` keeps on-demand
allocation (§18.1): pooling is a test-runner (and later server) choice,
not a global one.

### 21.4 Per-test timeouts through epochs

As §19.4 specifies, `report_timeout(millis)` sets the case store's
epoch deadline, and the supervisor arms a reactor timer for the same
instant: a busy loop traps on the epoch check, a parked host wait is
cut by the timer, both report `time-limit`. One epoch-increment thread
per `hd test` process ticks all worker stores (the W1 engine already
enables `epoch_interruption`); `--time-limit` sets the same deadline
for cases without `timeout`. The 50 ms supervisor grace of `test.mjs`
goes away with the worker processes: in-process, the trap is observed
at the next `poll`, so timeout accounting is exact, not grace-padded.
A timed-out case's store is dropped (slot reset), never reused
mid-flight.

### 21.5 Panic frames via the M6 line table

On a trap or a failing `expect_panic` mismatch, the runner reads the
case through wasm-layout.md §15.9: category global + message buffer,
`hd.sites` at the caller frame, backtrace frames through `hd.lines` /
`hd.names`, panic site first, rendered as the `cli.test.report.frame`
`FILE:LINE FUNCTION` lines. The section tables are parsed once per
test program (not per case) and shared across workers; per-case cost
is the anchor resolution (microseconds, trap path only). Property
shrink replays reuse the same parsed tables. This replaces the
`panicMessage` stderr-scraping of `test.mjs`.

### 21.6 Conformance runtime tiers in-process

The driver conformance runner (`run_wasm`, `run_tests`) and the CLI
tier stop spawning one `node` process per case: with
`HD_CONFORMANCE_ENGINE=wasmtime` (W1 precedent: 129 programs, 128
identical) they instantiate the built module in-process on the same
`--jobs` pool (§18.7 "Left for W2" closes the five runtime-profile
programs once §21.2 lands them). Expected saving: one process spawn
(~40 ms, M2) per case per pass — the suite's slowest gate (M1) gets
faster without changing a verdict: outputs are compared byte for byte
before the switch flips, as W1 did for `hd run`. Node stays as the
differential oracle until the outputs match for a full pass.

### 21.7 Pricing

Anchors (measured §18.7, aarch64 macOS): process start 41 ms Node vs
7 ms wasmtime; steady compute at parity or better except
closure-heavy code (3.6x slower under Cranelift `OptLevel::None` —
matters for property bodies that are all closures).

| Choice | Startup per test | Memory per instance | Parallelism |
| --- | --- | --- | --- |
| In-process wasmtime, pooling, `InstancePre` (§21.2–21.3) | ~0.1 ms (§19.3 budget: slot + instantiate + init) vs ~40 ms `node` spawn | slots × per-slot maxima, bounded; constants per instantiation as §19.1 prices | `--jobs` workers × one `Store` each; properties pin one worker (§19.4 D3) |
| Epoch timeouts (§21.4) | one shared ticker thread; zero per-case threads (vs one worker process per case today) | none per case | exact accounting, no 50 ms grace padding |
| Parsed line tables shared per program (§21.5) | once per program (~ms) | one parsed table per program across workers | trap path only; clean runs untouched |
| Conformance in-process (§21.6) | removes ~40 ms spawn × cases × 2 passes | test `hd` process only | same pool sizing as M1 (`HD_TEST_JOBS`/8) |

**Questions.** Q1: three linkers, or one linker with optional imports
resolved to traps? Three (recommended): a kind mismatch must fail at
build time, and `InstancePre` needs exact imports. Q2: pool sizing —
slots = workers, or workers + shrink spare? Plus spare (recommended):
a shrinking property otherwise stalls its worker. Q3: keep the Node
runner behind a flag after the switch? Yes until one green full pass
on wasmtime, then delete (recommended): the differential is the only
proof the outputs match. Q4: `OptLevel::None` closure gap — accept,
or move tests to `Speed`? Accept for W4 (recommended): correctness
first; `test-latency` will say whether properties need the optimized
pipeline (`hd test --release` already pairs test profile with it).

Recommendation: build in §21.1 → §21.2 → §21.3 order (providers, then
linkers, then pooling), flip conformance tiers (§21.6) before making
wasmtime the `hd test` default, and keep Node until the differential
pass is green.

## 22. Resource Limits On wasmtime (M15, 2026-10-10, #269)

The spec's Resource Limits (`cli.limit.*`, `cli.memory.*`,
`flow.panic.heap-exhausted`, `flow.panic.time-limit`) have nothing
built: W1 sets epoch interruption with a far deadline, a fixed 4 MiB
stack, and a 64 MiB initial GC heap, and only the exchange-buffer
growth refusal panics `heap-exhausted` today. This designs the flags
through wasmtime, the panic or error each limit produces, and Node
behavior until W5. No code changes.

### 22.1 `--time-limit` through epochs

`--time-limit DURATION` sets the run's epoch deadline
(`Store::set_epoch_deadline`) on top of the shared ticker thread from
§21.4; a busy loop traps at the next back-edge check and the reactor
cuts a parked host wait at the same instant, exactly as
`report_timeout` does. The trap maps to `panic: time-limit`
(`flow.panic.time-limit`), never to a host `internal error`. In
`hd test` the flag applies per case per instance (`cli.limit.test`),
composing with (not replacing) `report_timeout`: the earlier deadline
wins. No flag means no deadline (`cli.limit.none`), as today. Invalid
durations are usage errors (`cli.limit.forms`).

### 22.2 `--max-heap` through `ResourceLimiter` and heap accounting

`--max-heap SIZE` caps the program's heap: linear-memory growth past
the cap is refused in `ResourceLimiter::memory_growing`, and the
resulting trap maps to `panic: heap-exhausted`
(`flow.panic.heap-exhausted`), the same category the exchange-buffer
refusal already produces. GC-heap growth counts against the same cap
through the pooling allocator's heap accounting; the exact accounting
call is an implementation check (Q2), but the observable contract is
fixed: any allocation past the cap panics `heap-exhausted`, in every
profile, with no default cap. `--max-memory` (compiler) is separate by
rule (`cli.memory.cap.program`): it stays a process-level cap (§22.3).

### 22.3 Compiler memory: `--max-memory` / `HD_MAX_MEMORY`

A counting global allocator tracks heap bytes held by checking and
building; past the cap the driver stops and reports one `memory-limit`
error naming its stage (`cli.memory.cap.error`). Heap-only accounting
is documented as approximate (RSS includes stacks, mapped `cwasm`, and
the engine); platform RSS sampling is explicitly out of scope. Off
unless asked (`cli.memory.cap.default`), since scheduling makes the
trip point nondeterministic by rule. The env var is the fallback when
no flag is given.

### 22.4 Stack and the remaining panics

`max_wasm_stack` (profile's limit, W1's 4 MiB default) already traps as
`stack-exhausted`. No new stack flag: the profile owns the limit
(§18.1). Fuel stays off (simulation, Later).

### 22.5 Node until W5

Node keeps what it has: `report_timeout` per test case (grace-padded,
§21.4) and no heap cap (V8 heap; an over-allocation kills the process
instead of panicking). `--time-limit` on Node is approximated by the
existing supervisor termination (a kill, not a `time-limit` panic);
`--max-heap` on Node is accepted and ignored with a warning until W5,
since V8 exposes no per-instance heap cap. wasmtime is the precise
path; the flags' parsing and forms are engine-independent from day
one, so CLI cases assert behavior only where the engine can deliver
and skip otherwise.

## Questions with a recommendation

- **Q1. Epoch granularity for short limits?** Keep the §21.4 shared
  ticker (recommended): per-run timer threads would multiply with
  `--jobs`; sub-10 ms limits are inherently approximate.
- **Q2. GC-heap accounting call?** Confirm at implementation whether
  the pooling allocator exposes GC bytes directly or needs sampling
  (recommended: take whichever exists in wasmtime 49; the panic
  contract does not depend on it).
- **Q3. Warn or reject `--max-heap` on Node?** Warn and ignore
  (recommended): the flag parses everywhere, but failing a run Node
  cannot limit would break the differential period.
- **Q4. Counting allocator vs RSS sampling for `--max-memory`?**
  Counting allocator (recommended): portable, deterministic enough for
  a best-effort cap the spec already calls nondeterministic.

Recommendation: implement §22.1–22.2 on wasmtime first (both are pure
driver/host work on the W1 engine), then §22.3, keeping the Node
approximations of §22.5 until W5.
