# New Compiler Design: Runtime And Host Interface

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

The ABI table is `hd_host_abi`; the engine-neutral embedding API, grants
and the test model are `hd_run`; the wasmtime engine and its native
providers are `hd_run_wasmtime`; the JS host is `host/*.mjs`.

## 16. Runtime

### 16.1 Where Each Piece Lives

The rule is the repository's: library code is hd in `lib/std`; the
compiler emits only what the spec names as intrinsic or what only it can
build.

| Piece | Where | Notes |
| --- | --- | --- |
| `Set`, `Deque`, `Heap`, `ListView`, string operations, formatting, `Debug`, `Display`, JSON, iterators | hd in `lib/std` | over `Array[T]` and string intrinsics; `Deque` and `Heap` store `T?` in `List` slots, laid out as in crates/hd_wasm/DESIGN.md §15.2 |
| `Map` | hd in `lib/std`: a compact insertion-ordered table | Implementation differs: `Map`'s table is runtime helpers the emitter writes (`hd_wasm/src/map.rs`), which is worse: library code in the compiler. See #180 |
| `Choices`, shrink-free generators, structured assert diffs | hd in `lib/std` (`std.testing`) | the runner shrinks on the host side |
| `block_on`, `all!`, `race!`'s frame, `Waker` | compiler intrinsics | the spec names them so ([Standard Combinators](../../spec/lang/11-requirements-and-suspension.md#standard-combinators)); `race!` itself is hd in `std.task` |
| default-profile provider types (`HostConsole`, `HostFs`, ...) | hd declarations in `lib/std` whose method bodies are intrinsic | an intrinsic body maps to a host import by the ABI table (§17.1); the signature is plain hd |
| frames, `all!` and `race!` frames, vtables, closure environments, entry wrappers, init sequencing, panic stubs, check sequences | generated | §12 to §15 |
| boundary encoders and decoders | generated per boundary type | like a derive, from the type's structure (§17.4) |
| `Array[T]` operations, string byte access, `dbg`, panics | intrinsics (`hd_host_abi::INTRINSICS`, lowering `Compiler`) | a few Wasm instructions each |
| float text, float parsing, case mapping | host primitives (`hd:prim`, §17.2) | the spec makes them host work |
| capability methods, `hd:rt` (block, abort, stderr, stdout, panic), `TestRunner`, `PropertyRunner` | host | §17 |

Planned, not built: `Hasher` with `write_u8` to `write_u64` and
`write_str` default methods (a std and spec change), so an integer or
string key hashes with no allocation.

### 16.2 Panics And Exit Codes

- A panic traps (§15.5). The host reports the category, the message, the
  location and the backtrace on standard error.
- `hd run` and `hd FILE` exit with the profile's panic status, which is
  never 101
  ([`module.profile.panic-status`](../../spec/lang/10-modules.md#r-module.profile.panic-status)).
- An entry that returns `.Err(e)` exits with status 1. The entry wrapper
  renders `e` and its cause chain in Wasm and writes them through the
  `hd:rt` stderr import. That import is not a capability, so a program
  whose row lacks `Console` can still report its error.
- wasmtime reports two traps that Node cannot as panics: a stack
  overflow is `stack-exhausted` and a refused heap allocation is
  `heap-exhausted`; a passed wall deadline is `time-limit`.
- An internal error (a trap with no category) exits with status 101, as
  `hd`'s own failures do. Implementation differs: both hosts print
  `internal error: ...` and exit with the panic status; not judged.
- A test case's panic fails the case, or passes it under a matching
  `expect_panic` (§19.4).

### 16.3 Allocation

- Every hd object is a Wasm GC struct or array. There is no allocator in
  linear memory.
- The exchange buffer (§17.3) is the only linear memory, and only modules
  that cross structured values have one.

Planned, not built: a `Hook(Alloc)` at each `struct.new` and
`array.new` site, emitted as a counter in a stats build, so the
`allocations` metric has a count on wasmtime.

### 16.4 Metadata And The Import List

**The import list** is the only source of a module's capability needs
([`cli.cap.total.needs`](../../spec/cli/command-line.md#r-cli.cap.total.needs);
`hd_run::needs`). Each capability trait's methods are imported from
module `hd:<Key>`, with `Key` as in the grant table (`hd:Console`,
`hd:FsRead`, `hd:Http`). Non-capability imports use `hd:rt`, `hd:prim`,
`hd:TestRunner`, `hd:PropertyRunner` and, in hook builds, `hd:hook`.

`hd FILE.wasm` treats a module with an import outside the ABI table
(`hd_host_abi::is_known_import`) as not built by `hd`
([`cli.wasm.not-hd.detect`](../../spec/cli/command-line.md#r-cli.wasm.not-hd.detect)).

Planned, not built: an `hd.runtime` custom section (format, compiler
build id, entry kind, the test list, the exchange export), with a module
lacking it also treated as not built by `hd`. `hd_wasm/src/meta.rs`
defines `RuntimeMeta` and its codec; link does not write it.

## 17. Host Interface And Embedding

### 17.1 One ABI Description

`hd_host_abi` holds one table, written in Rust:

```rust
pub struct HostTrait { pub key: &'static str, pub std_path: &'static str, pub methods: &'static [HostMethod] }
pub struct HostMethod {
    pub name: &'static str,             // "read_text"
    pub params: &'static [Codec],       // Void | Scalar(I32 | I64 | F64) | Buffer(std type name)
    pub result: Codec,
    pub wait: Wait,                     // Never | May: start-and-poll
    pub resource: Option<ResourceArg>,  // which argument the grant checks: Path | Host | Addr | EnvName | Program | SysName
}
pub static TABLE: &[HostTrait];         // capability traits
pub static RUNNERS: &[HostTrait];       // TestRunner, PropertyRunner
pub static HANDLES: &[HostTrait];       // Net's TcpStream, TcpListener, UdpSocket
pub static INTRINSICS: &[(&str, Lowering)];  // Compiler | HostPrimitive(name)
```

Four things are generated from it:

1. the hd-side bodies of std's provider methods (each intrinsic body
   becomes an import call with the encoders it needs);
2. wasmtime host stubs that decode arguments, check the grant, call the
   provider and encode results;
3. the JS glue's import object, codecs and grant checks (§17.10);
4. the list of known import names for `hd FILE.wasm`'s checks.

Std's capability traits are still declared in `lib/std` in hd. The table
mirrors them, and a test fails when the two disagree.

Implementation differs: only items 1 and 4 come from the table (`hd_wasm`
generates the hd-side stubs; `is_known_import` reads it). The wasmtime
providers (`hd_run_wasmtime/src/host/`) and the JS host
(`host/core.mjs`) are written by hand, and `generate_js_glue` returns
"not implemented". That is worse: three hand-kept copies of every codec
and grant check can drift apart, and only tests catch it.

**A runtime profile's own traits.** An entry row's trait that the table
does not list is a selected runtime profile's own, as a conformance
fixture's `Gauge`, `Sensor` or `Vault`
([Runtime Profiles](../../spec/conformance/README.md#runtime-profiles)).
`hd_wasm` derives its row from the declaration: imports from
`hd:<Trait>` by method name, a wait when the method suspends, and every
argument and result in the exchange buffer, so each result gets the
checks of its declared type. Their providers are fixture code:
`host/profiles.mjs` on Node, `hd_run_wasmtime/src/profiles.rs` on
wasmtime, never enabled by the default host.

### 17.2 Import Shapes

| Method kind | Import | Example |
| --- | --- | --- |
| scalars only, never waits | one function with Wasm parameters and results | `hd:Clock` `now() -> i64` |
| structured values, never waits | `(len: i32) -> i32`: arguments in the exchange buffer, result length back | `hd:Env` `get` |
| may wait | `name.start(len) -> (i32 status, i32 value)`, then `name.finish(h) -> i32` | `hd:FsRead` `read_text.start`, `read_text.finish` |
| runtime | `hd:rt` `block() -> i32`, `abort(h)`, `stderr(len)`, `stdout(len)`, `panic()` | |
| host primitive | `hd:prim` `name`, no receiver, never waits | `format_f64` |

A `.start` call returns two values. Status `0` means the operation
finished at once: the result is in the buffer and `value` is its length.
Status `1` means it is pending and `value` is its handle. The leaf frame
registers its waker in the wake table under the handle and returns
Pending. When the handle appears in an `hd.wake(n)` batch, the next poll
calls `.finish(h)`, which writes the result into the buffer and returns
its length. The generated decoder checks every length against the
buffer's current size before reading. Imports use Wasm's module and
name fields (`hd:FsRead`, `read_text.start`), so the startup check reads
the module field alone.

**Handles.** A handle is instance-scoped and generation-tagged: 20 bits
of slot and 11 bits of generation, so it is never negative
(`hd_host_abi::Handle`). The host keeps one state per slot:

| State | Entered by | Then |
| --- | --- | --- |
| Pending | `.start` returning status 1 | completion moves it to Completed; `hd:rt/abort(h)` to Cancelled |
| Completed | the operation finishing | its handle is queued for the next `hd.wake` batch; `.finish(h)` frees the slot |
| Cancelled | `abort(h)` | a late completion is dropped; the slot is freed at once |

A freed slot's generation increments, so a stale handle never names a
new operation. The Wasm wake table stores the full handle and ignores a
wake whose generation does not match. `.finish` or `abort` on a stale
handle is a `host-contract` panic.

**Host primitives.** A key of `INTRINSICS` whose lowering is
`HostPrimitive(name)` is one import `hd:prim` `name`. `hd_wasm` lowers
its call as `Helper::HostPrim`: scalar arguments are import parameters
(an `f32` or `f64` is the raw value, a `usize` an `i32`), a string
argument is written in the exchange buffer with its byte length as the
parameter, a string result is decoded from the buffer like a provider's,
and a scalar result (`parse_f64`) is the import's own result. A program
that never calls one has no such import. Six exist: `format_f64`,
`format_f32`, `format_f64_fixed`, `string_lower`, `string_upper` and
`parse_f64`. Both hosts write the same text from the exact significand
and exponent: `host/float.mjs` with `BigInt`, `hd_run_wasmtime/src/prim.rs`
with its own big integers. No result depends on
`Number.prototype.toString` or `toFixed`:

- *Shortest text* is Steele and White free-format printing in Burger and
  Dybvig's form. The interval of values that read back as the float
  reaches halfway to each neighbor at the float's own width (`f32` or
  `f64`), narrower below a power of two, and includes its ends exactly
  when the significand is even. Of the shortest digit strings in it, the
  closest is chosen, and an exact tie goes to the even last digit (as
  JavaScript, Python and Go do; Rust takes the larger, and the spec does
  not say). The notation rules of `types.display.notation` follow.
- *Fixed-point text* is the exact quotient of `significand * 10^digits` by
  a power of two, rounded half to even, with the sign bit kept.
- *Case mapping* is Unicode Default Case Conversion with full mappings,
  as the spec asks (`ß` gives `SS`, a final sigma by context).
- *Parsing* reads the exact rational `digits * 10^exponent` and rounds it
  to the nearest `f64`, a tie to the even significand, with infinity past
  the range and `0.0` below it. It does not use `Number`, which the
  ECMAScript text lets round after the twentieth digit.
- The import is the smallest lowering that matches the spec ("Host. The
  host supplies these, for now."); a generated Wasm helper would need
  Ryu or Grisu tables and a big-number fallback.

`crates/hd_driver/tests/all/float_text.rs` checks the host text of
about 13,000 `f64`, 5,600 `f32` and 14,000 fixed-point cases against
Rust's own `{:e}` and `{:.N}` printing.

### 17.3 The Exchange Buffer

- One exported linear memory, `hd.x`, of one page at first, grown on
  demand. A module that crosses only scalars has none. The writer grows
  it: generated Wasm code before writing arguments, the host before
  writing a result. A refused growth, which `memory.grow` reports as
  `-1`, is the `heap-exhausted` panic on either side (§17.8).
- The caller writes encoded arguments at offset 0 and passes their length.
  The host writes the encoded result at offset 0 and returns its length.
- Calls do not nest: an instance runs on one thread, and no host call
  calls back into Wasm. `block` returns before any wake is processed, so
  one buffer is enough.
- **Strings and byte lists** cross as raw bytes, copied by a generated
  Wasm loop (Wasm GC has no bulk copy between arrays and memory). A
  string crosses with no conversion
  ([`types.string.host-bytes`](../../spec/lang/04-type-system.md#r-types.string.host-bytes)).
  A string is a view (crates/hd_wasm/DESIGN.md §15.2): Wasm to host
  copies only the viewed range, so a slice crosses at its own size,
  never its source's. Host to Wasm builds one exact-size array, viewed
  from start 0. The host reads the bytes in one slice. The loop reads
  and writes 8 bytes per step, then the tail one byte at a time.

Planned, not built: a string literal passed straight to a host call
copied from the module's data segment into the buffer with
`memory.init`, with no GC array (crates/hd_wasm/DESIGN.md §15.4).

### 17.4 Encoding Structured Values

A compact binary format, with both sides generated from the same type
description, so it needs no field names. `hd_wasm::boundary` describes
each boundary type as a `BTy` (its parts and Wasm layout, no pool) and
generates one encoder `(off, values...) -> off'` and one decoder
`(off) -> (values..., off')` per `BTy`, as `rt` helpers, so methods that
cross the same type share them. The JS host writes the same bytes with
its `Enc` and `Dec` classes.

| Value | Encoding |
| --- | --- |
| integers | LEB128; zigzag for signed; wider than 32 bits is an `i64` |
| floats | raw IEEE 754 little-endian bytes |
| `bool` | one byte, 0 or 1 |
| `char` | its scalar value |
| `string`, `List[u8]` | length, then bytes (for a string, the viewed range only) |
| list | count, then elements |
| data, newtype | fields in declaration order; a newtype is data of one field, so `Path` is its string's bytes |
| enum | variant index, then payload fields |
| `Option` | 0, or 1 and the value |
| tuple | elements |
| host handle | its handle number, unsigned LEB128 |
| data with a field that is not `pub` | its JSON text, written by its `Serialize` and read by its `Deserialize` ([`module.boundary.serialize.exact`](../../spec/lang/10-modules.md#r-module.boundary.serialize.exact)) |

- Only boundary-safe types cross, as the spec defines them.
- **Arguments.** A method's structured arguments are encoded one after
  another at offset 0, with no count; the import's last parameter is
  their total length. Scalar arguments stay Wasm parameters, before it.
  A method whose only argument is a `string` passes the string's bytes
  alone, with no length prefix: the parameter is the length
  (`Console.write_line`, `Env.get`, `TestRunner.snapshot_check`).
- **Results.** A waiting method's result is always in the buffer. A
  method that never waits returns a scalar as its Wasm result, a data
  value of one `i64` field (`Timestamp`, `Instant`) as that `i64`, and a
  structured value in the buffer, with its length as the result.
- **An enum of tags** (each payload at most one payload-free enum, as
  `Result[void, ConsoleError]`) is read as one or two bytes, which are
  the general encoding's bytes for fewer than 128 variants.
- **Checks.** The encoder grows the buffer before it writes (a refused
  growth is `heap-exhausted`). The decoder checks every LEB128 byte and
  every length against the buffer's size, a variant index against the
  variant count, an option's tag against 1, and a narrow integer against
  its range; a failure is the `host-contract` panic.
- **Host handles.** A handle trait (`hd_host_abi::HANDLES`) becomes a
  trait value whose payload is a struct of the number and whose vtable's
  slots (`Helper::HandleSlot`) call `hd:Net` `<Trait>.<method>` with the
  number first. The host keeps a closed handle's number, so every later
  call on it is `Disposed`
  ([`std-net.handles.closable`](../../spec/std/net.md#r-std-net.handles.closable)).
- **Nesting.** Boundary types nest at most 32 deep, and recursive types
  do not cross, so a crossing value has no cycle
  ([`module.boundary.cycle`](../../spec/lang/10-modules.md#r-module.boundary.cycle)).
  Maps, functions and other trait values do not cross yet; a host method
  over one stops at Link as unsupported.

The structured values of the ABI table, byte by byte:

| Value | Bytes |
| --- | --- |
| `string`, `Path` | length, then the UTF-8 bytes |
| `List[u8]` | length, then the bytes |
| `string?` | `0`, or `1` and the string |
| `List[string]` | count, then each string |
| `Entry` | its path, its kind's index (`File` 0, `Directory` 1, `Symlink` 2), its size |
| `Entry?` | `0`, or `1` and the entry |
| `List[Entry]` | count, then each entry |
| `Result[T, E]` | `0` and the `T`, or `1` and the `E` |
| `FsError` | the variant index (`NotFound` 0, `PermissionDenied` 1, `NotGranted` 2, `AlreadyExists` 3, `NotADirectory` 4, `IsADirectory` 5, `InvalidUtf8` 6, `Other` 7), then its path, or `Other`'s message |
| `ConsoleError` | `0` (`Closed`) |
| `ProcessOutput` | stdout, stderr, then the status, zigzag |
| `ProcessError` | the variant index (`NotFound` 0, `PermissionDenied` 1, `NotGranted` 2, `Other` 3), then `Other`'s message |
| `SysError` | the variant index (`NotGranted` 0, `Unsupported` 1), then its name |
| `Request` | its method's index (`Get` 0 to `Options` 6, then `Other` 7 and its name), its URL, its header pairs (a count, then each name and value), its body, and its timeout as a `Duration?` (a `Duration` is its milliseconds, zigzag) |
| `Response` | its status, its header pairs, its body |
| `NetError` | the variant index (`NotGranted` 0, `InvalidAddress` 1, `Dns` 2, `Refused` 3, `Other` 4), then its text |
| `ResourceError[NetError]` | `0` (`Operation`) and the `NetError`, or `1` (`Disposed`) |
| `Datagram` | its bytes, its host, its port |
| `HttpError` | the variant index (`NotGranted` 0, `InvalidUrl` 1, `Dns` 2, `Connect` 3, `Tls` 4, `Timeout` 5, `TooManyRedirects` 6, `Other` 7), then its text, which `Timeout` lacks |

So `Result[string?, ConsoleError]` is `0 0` at the end of input, `0 1`
and the line for a line, and `1 0` for `Closed`;
`Result[List[Entry], FsError]` is `0`, the count and the entries, or `1`,
the variant index and the path.

### 17.5 Host Calls On wasmtime

- Host methods are plain synchronous Rust functions (`Linker::func_wrap`).
  wasmtime's async support is off: with start-and-poll, no Wasm call ever
  waits inside the engine, so no fiber stacks are needed, and each call
  stays a plain native call.
- **The `hd-io` runtime** is one process-wide tokio current-thread
  runtime, driven on a dedicated `hd-io` thread and started by the first
  offloaded operation (`io_runtime.rs`). A `.start` that cannot finish at
  once spawns a task there and returns its handle; the task delivers its
  result into its instance's reactor inbox (`reactor.rs`). Timers,
  `Console.write_line` and standard-input lines complete through the same
  inbox. One runtime per process, not one per worker thread, is better:
  a program that never offloads starts no thread, and workers share one
  I/O driver.
- **The driver loop** (§14.4) runs in `hd_run_wasmtime`: poll, then wait
  on the reactor's condvar for the next completion (a timer's deadline or
  a delivery), then wake and poll again. **`hd:rt/block`** waits the
  same way inside a host call (§14.9).
- **Abort**: `hd:rt/abort(h)` aborts the task; HTTP requests and timers
  stop for real
  ([`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
  A blocking special-file operation (a FIFO) only has its result dropped.
- **Path sandbox.** Granted directories are opened once as `cap-std`
  directory handles, and every file operation resolves relative to them,
  which removes a check-then-use race on symbolic links, as the host
  capabilities spec asks (`fs/sandbox.rs`). A handle is opened at its
  first use, from the file system root without following any link, and
  kept for the run. An operation on a granted entry itself goes through
  the entry's parent by name and never follows a link there. A `remove`
  or `rename` that takes a granted directory away drops its handle, so a
  directory made again at that path is opened afresh. A trait with no
  limit acts by name with `std::fs`.

### 17.6 Host Calls In The Browser

- **Start and poll.** The JS host starts a Promise (fetch, a timer), keeps
  it in a handle table, and returns the handle. On settle it queues the
  handle in one completion queue. While the glue is Idle, a non-empty
  queue schedules one task that calls `hd.wake(n)` and then
  `hd.poll()`, so wakes coalesce per task turn.
- **One entry at a time.** The glue keeps an entry state: Idle, Running,
  or Suspended (JSPI, inside `hd:rt/block`). Only Idle starts an export.
  While Suspended, a completion resolves the Promise that the suspended
  `block` import waits on, and nothing else; `block` then writes the
  queued handles and returns. While Running or Suspended, completions
  only queue. So no export re-enters a Wasm stack that JSPI suspended.
- **Cancellation** calls `AbortController.abort()` for fetches and
  `clearTimeout` for timers.
- **`block_on`**: with JSPI, `hd:rt/block` is a suspending import and
  the exports are wrapped with `WebAssembly.promising`. Without JSPI, a
  module that imports `hd:rt/block` runs in a restricted profile where a
  `.start` inside `block_on` must finish synchronously (a same-origin
  synchronous `XMLHttpRequest`), and any other pending operation panics
  `host-contract` naming JSPI.

Implementation differs: `host/web.mjs` runs the program in a dedicated
worker and answers every host call synchronously: `Http.send` is a
synchronous `XMLHttpRequest` that finishes inside `.start`, and only
`Clock.sleep` and `Console.write_line` complete later. Not judged: it
is far simpler and needs no JSPI, but an HTTP request cannot be
cancelled and holds the worker until it ends.

### 17.7 Capability Grants

- **Startup refusal.** Before instantiation, the host reads the import
  list, maps each `hd:<Key>` module to its trait, and refuses with status
  101 when a needed trait is totally denied, naming the setting
  ([Total Deny](../../spec/cli/command-line.md#total-deny);
  `hd_run::check_grants`). Nothing runs before the check, not even init.
- **Partial deny.** The provider extracts the method's resource argument,
  checks it against the grant (`hd_run::Grants`), and on a refusal
  encodes the method's `NotGranted` variant as the result. The program
  sees an ordinary `.Err`
  ([`cli.cap.partial.refuse`](../../spec/cli/command-line.md#r-cli.cap.partial.refuse)).
  `Env.get` returns `.None` and the host prints the one-line notice once
  per name.
- Checks live in host providers only, never in hd code, as the host
  capabilities spec requires.
- **On Node,** `hd run` and `hd FILE` write the run's grant and its
  `Args.program` into the run host's configuration; `hd test` writes each
  case's grant into the test host's. Every provider of `host/core.mjs`
  asks `createGrant`'s checks before it touches a resource, and `rename!`
  checks both paths
  ([`cli.cap.scope.rename`](../../spec/cli/command-line.md#r-cli.cap.scope.rename)).
  The test runner's `Process` provider starts the package's executables
  and tasks, built by `hd test` with the package's `[capabilities]` table
  ([`cli.cap.source.hd-run`](../../spec/cli/command-line.md#r-cli.cap.source.hd-run)),
  in the package directory.
- **`Http` on Node.** `send!` finishes inside its `.start`: the program's
  thread hands the request to a client thread (`host/http.mjs`) and waits
  on a shared flag, so `block_on` never needs the event loop. The client
  checks the grant for the URL and for each redirect target
  ([`cli.cap.scope.redirect`](../../spec/cli/command-line.md#r-cli.cap.scope.redirect))
  before it connects.
- **`Net` on Node.** Each instance has a client thread of its own
  (`host/net.mjs`) that holds its sockets and is stopped when the
  instance ends. Every operation finishes inside its `.start`: `read!`,
  `accept!` and `receive!` hold the program until data, a connection or a
  datagram arrives. The program's thread checks the host's form and the
  grant
  ([`cli.cap.scope.net`](../../spec/cli/command-line.md#r-cli.cap.scope.net),
  [`std-net.send-to.grant`](../../spec/std/net.md#r-std-net.send-to.grant))
  before it posts an operation.

### 17.8 Resource Limits

| Limit | Mechanism | Failure |
| --- | --- | --- |
| time (`--time-limit`, a test's `timeout`) | epoch interruption: one `hd-epochs` thread per process, started only when a store has a deadline, increments every registered engine's epoch every 10 ms; the store's epoch callback checks the wall clock, which stays authoritative | `time-limit`; a pending host wait is cut at the same deadline |
| initial GC heap size | 64 MiB for an uncapped `hd run`; 0 when a heap cap is set and for `hd test`'s heaps, which start empty and grow on demand, so a small cap is not used up before the program allocates. wasmtime's copying collector otherwise re-copies a moderate long-lived set on every collection without growing the heap | none; it only sets where growth starts |
| GC heap, the exchange buffer and host-held data (`--max-heap`) | one `Ledger` per program, enforced through a `ResourceLimiter` for the store's backing bytes; host operations, including tasks on `hd-io`, charge the same ledger (`std-host.op.heap`). A refused linear-memory growth makes `memory.grow` return `-1`, which the writer turns into `heap-exhausted` (§17.3); a GC allocation that fails after a collection traps | `heap-exhausted` |
| stack | `Config::max_wasm_stack`, 4 MiB by default | `stack-exhausted` |
| instances, tables, memories, GC heaps in `hd test` | the pooling allocator's totals, one slot per worker plus shrink spares. The runner never runs more stores at once than the pool has slots, so exhaustion means a bug | an internal error naming the limit |

One ledger for the store and host-held data replaces a design with a
separate host budget; it is better, since the spec counts both against
the one heap limit.

**Test timeouts on Node.** A test case's `timeout` is evaluated first in
its instance and reported with `TestRunner.report_timeout`
([`std-testing.runner.timeout`](../../spec/std/testing.md#r-std-testing.runner.timeout)).
The JS host fails a body that ends after its deadline, and cuts a host
wait (`block`, the driver's wait) at the deadline, with `time-limit`. V8
cannot interrupt a Wasm loop from its own thread, so `host/test.mjs` runs
the cases in a worker thread: its main thread stops a worker whose body
is still running 50 ms past the deadline, reports that run as a
`time-limit` panic, and starts a new worker at the next row or case.

### 17.9 The Embedding API

The CLI is a host on this API; the test runner and later embedders are
others. It lives in `hd_run`:

```rust
pub trait Engine: Send + Sync {                       // wasmtime natively; Node through hd_cli
    type Module: Send + Sync;
    fn load(&self, wasm: &[u8], cache: &dyn CacheStore) -> Result<Self::Module, LoadError>;
    fn instantiate(&self, m: &Self::Module, host: &HostSetup) -> Result<Box<dyn Instance>, StartError>;
}
pub trait Instance {
    fn init(&mut self) -> Outcome;                    // runs hd.init
    fn poll(&mut self) -> Poll<Outcome>;              // runs hd.poll
    fn wake(&mut self, handles: &[u32]);
    fn call_test(&mut self, export: u32) -> Poll<Outcome>;
    fn completed(&mut self) -> Vec<u32>;
}
pub trait Provider: Send + Sync {
    fn key(&self) -> &str;
    fn call(&self, method: u32, args: &[u8], out: &mut Vec<u8>, cx: &mut CallCx) -> CallResult;  // Ready | Pending
}
pub struct HostSetup { pub providers: Vec<Box<dyn Provider>>, pub grants: Grants, pub limits: Limits, pub args: Vec<String>, pub program: String }
pub enum Outcome { Exit(u8), Panic(PanicReport), Internal(String) }
```

- `hd run` builds a `HostSetup` from the package's grant. `hd test`
  builds one per test kind (§19.3).
- A later embedder supplies its own providers for its own capability
  traits through the `Provider` trait, with their ABI descriptions at run
  time instead of from the static table (#51).
  Implementation differs: the wasmtime engine binds its native providers
  directly in its linker (`hd_run_wasmtime/src/host/`) and does not call
  `HostSetup::providers`; not judged, since no embedder exists yet.
- wasmtime compiled modules are cached in process only, keyed by their
  bytes; wasmtime's serialized modules are `unsafe` to deserialize, so
  there is no cross-run artifact cache (`cache.rs`).

### 17.10 The Generated JS Glue

One ES module, generated from `hd_host_abi` and checked in:

- `makeImports(module, providers, grants)` returns the import object for
  exactly the module's imports;
- the codecs of §17.4, reading and writing the exchange buffer through a
  `DataView`, and strings through `TextDecoder` and `TextEncoder`;
- the poll and wake loop of §17.6;
- panic decoding: reads the panic globals and the buffer, maps
  `wasm-function[i]:0xOFF` frames through `hd.lines`;
- a provider interface; the playground supplies browser providers.

It uses only web platform APIs, so it also runs under Node.

Implementation differs: the glue is hand-written. `host/core.mjs` holds
the import object, codecs, platform-neutral providers and the entry
driver; `host/node.mjs` and `host/web.mjs` add platform providers;
`host/run.mjs`, `host/test.mjs` and `host/cases.mjs` drive runs and
tests. That is worse for the same reason as §17.1: the codecs are a
second hand-kept copy of the table.
