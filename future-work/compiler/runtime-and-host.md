# New Compiler Design: Runtime And Host Interface

Part of the [compiler design](README.md).

## 16. Runtime

### 16.1 Where Each Piece Lives

The rule is the repository's: library code is hd in `lib/std`; the
compiler emits only what the spec names as intrinsic or what only it can
build.

| Piece | Where | Notes |
| --- | --- | --- |
| `List`, `Map`, `Set`, `StringBuilder`, string operations, formatting, `Debug`, `Display`, JSON, iterators, `retry!`, `all_list!` | hd in `lib/std` | over `Array[T]` and string intrinsics |
| `Choices`, shrink-free generators, structured assert diffs | hd in `lib/std` (`std.testing`) | the runner shrinks on the host side |
| `block_on`, the waker type, the wake table | hd in `lib/std` (`std.task`) | over one `hd:rt` import |
| default-profile provider types (`HostConsole`, `HostFs`, ...) | hd declarations in `lib/std` whose method bodies are intrinsic | an intrinsic body maps to a host import by the ABI table (§17.1); the signature is plain hd |
| frames, `all!` and `race!` frames, vtables, closure environments, entry wrappers, init sequencing, panic stubs, check sequences | generated | §12 to §15 |
| boundary encoders and decoders | generated per boundary type | like a derive, from the type's structure (§17.4) |
| `Array[T]` operations, string byte access, float bits, bit counts | intrinsics | a few Wasm instructions each |
| capability methods, `hd:rt` (block, abort, stderr), `TestRunner`, `PropertyRunner` | host | §17 |

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
- An internal error (a trap with no category) exits with status 101, as
  `hd`'s own failures do.
- A test case's panic fails the case, or passes it under a matching
  `expect_panic` (§19.4).

### 16.3 Allocation

- Every hd object is a Wasm GC struct or array. There is no allocator in
  linear memory.
- The exchange buffer (§17.3) is the only linear memory, and only modules
  that cross structured values have one.
- **Allocation counts (mine).** Each `struct.new` and `array.new` site
  carries a `Hook(Alloc)`. A stats build (`hd run --stats`, Later)
  emits it as a counter increment. That gives the `allocations` metric a
  count on wasmtime, where V8's heap statistics do not exist (§23.2,
  inconsistency 6).

### 16.4 Metadata And The Import List

**The import list** is the only source of a module's capability needs
([`cli.cap.total.needs`](../../spec/cli/command-line.md#r-cli.cap.total.needs)).
Each capability trait's methods are imported from module `hd:<Key>`, with
`Key` as in the grant table (`hd:Console`, `hd:FsRead`, `hd:Http`).
Non-capability imports use `hd:rt`, `hd:TestRunner`, `hd:PropertyRunner`
and, in hook builds, `hd:hook`.

**`hd.runtime`** is a custom section, as the prototype's is:

```rust
pub struct RuntimeMeta {
    pub format: u16,
    pub compiler: Hash128,              // build id; not part of the module's semantics
    pub entry: EntryKind,               // Main | MainBang | Tests | ReplInput
    pub tests: Vec<TestMeta>,           // test programs only (§19.2)
    pub exchange: Option<ExportName>,   // the exchange buffer export, when present
}
pub struct TestMeta { pub export: u32, pub name: Box<str>, pub kind: TestKind, pub file: u32, pub line: u32 }
```

`hd FILE.wasm` treats a module without `hd.runtime`, without the entry
exports, or with an import outside the ABI table as not built by `hd`
([`cli.wasm.not-hd.detect`](../../spec/cli/command-line.md#r-cli.wasm.not-hd.detect)).

## 17. Host Interface And Embedding

### 17.1 One ABI Description

`hd_host_abi` holds one table, written in Rust:

```rust
pub struct HostTrait { pub key: &'static str, pub std_path: &'static str, pub methods: &'static [HostMethod] }
pub struct HostMethod {
    pub name: &'static str,             // "read_text"
    pub params: &'static [Codec],       // Scalar(I32 | I64 | F64 ...) | Buffer(BoundaryTy)
    pub result: Codec,
    pub wait: Wait,                     // Never | May: start-and-poll
    pub resource: Option<ResourceArg>,  // which argument the grant checks: Path | Host | Addr | EnvName | Program | SysName
}
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

### 17.2 Import Shapes

| Method kind | Import | Example |
| --- | --- | --- |
| scalars only, never waits | one function with Wasm parameters and results | `hd:Clock` `now_ms() -> i64` |
| structured values, never waits | `(len: i32) -> i32`: arguments in the exchange buffer, result length back | `hd:Env` `get` |
| may wait | `name.start(len) -> (i32 status, i32 value)`, then `name.finish(h) -> i32` | `hd:FsRead` `read_text.start`, `read_text.finish` |
| runtime | `hd:rt` `block() -> i32`, `abort(h)`, `stderr(len)` | |

A `.start` call returns two values (Codex re-review N-B3). Status `0`
means the operation finished at once: the result is in the buffer and
`value` is its length. Status `1` means it is pending and `value` is its
handle. The leaf frame registers its waker in the wake table under the
handle and returns Pending. When the handle appears in an `hd.wake(n)`
batch, the next poll calls `.finish(h)`, which writes the result into
the buffer and returns its length. The generated decoder checks every
length against the buffer's current size before reading.

**Handles.** A handle is instance-scoped and generation-tagged: 20 bits
of slot and 11 bits of generation, so it is never negative. The host
keeps one state per slot:

| State | Entered by | Then |
| --- | --- | --- |
| Pending | `.start` returning status 1 | completion moves it to Completed; `hd:rt/abort(h)` to Cancelled |
| Completed | the operation finishing | its handle is queued for the next `hd.wake` batch; `.finish(h)` frees the slot |
| Cancelled | `abort(h)` | a late completion is dropped; the slot is freed at once |

A freed slot's generation increments, so a stale handle never names a
new operation. The Wasm wake table stores the full handle and ignores a
wake whose generation does not match. `.finish` or `abort` on a stale
handle is a `host-contract` panic.

The prototype names imports `hd:<trait>/<method>`. D2 splits that into
Wasm's module and name fields, so the startup check reads the module field
alone.

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
  The host reads the bytes in one slice.

This replaces the prototype's call per byte (F-558). A `list_dir!` with a
few names is one start call, one copy loop each way, and one finish call.

### 17.4 Encoding Structured Values

A compact binary format (mine), with both sides generated from the same
type description, so it needs no field names:

| Value | Encoding |
| --- | --- |
| integers | LEB128; zigzag for signed |
| floats | raw IEEE 754 little-endian bytes |
| `bool` | one byte |
| `string`, `List[u8]` | length, then bytes |
| list | count, then elements |
| map | count, then key and value pairs in iteration order |
| data | fields in declaration order |
| enum | variant index, then payload fields |
| `Option` | 0, or 1 and the value |
| tuple | elements |

- Only boundary-safe types cross, as the spec defines them.
- **Cycles** ([`module.boundary.cycle`](../../spec/lang/10-modules.md#r-module.boundary.cycle)):
  the encoder counts depth, and past depth 256 it keeps a stack of the
  references above it and fails with `boundary-cycle` on a repeat. Acyclic
  values below the depth pay nothing.
- Decoding a map calls the key type's `Eq` and `Hash` in Wasm, as the
  spec requires.

### 17.5 Host Calls On wasmtime

- Host methods are plain synchronous Rust functions (`Linker::func_wrap`).
  D2 does not turn on wasmtime's async support (mine): with start-and-poll,
  no Wasm call ever waits inside the engine, so no fiber stacks are
  needed, and each call stays a plain native call.
- **The reactor** is a tokio current-thread runtime per worker thread.
  A `.start` that cannot finish at once spawns a local task and returns
  its handle. The task's completion lands in the reactor's completion
  queue.
- **The driver loop** (§14.4) runs in `hd_run_wasmtime`: poll, then
  `block_on` the reactor for the next completion, then wake and poll
  again.
- **`hd:rt/block`** runs the same reactor loop nested inside a host call
  (§14.9).
- **Abort**: `hd:rt/abort(h)` aborts the task; HTTP requests and timers
  stop for real
  ([`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
- **Path sandbox.** Granted directories are opened once as `cap-std`
  directory handles, and every file operation resolves relative to them.
  This removes the prototype's check-then-use race on symbolic links, as
  HOST_CAPABILITIES asks.

### 17.6 Host Calls In The Browser

- **Start and poll.** The JS host starts a Promise (fetch, a timer), keeps
  it in a handle table, and returns the handle. On settle it queues the
  handle in one completion queue. While the glue is Idle, a non-empty
  queue schedules one task that calls `hd.wake(n)` and then
  `hd.poll()`, so wakes coalesce per task turn.
- **One entry at a time (Codex re-review N-B4).** The glue keeps an
  entry state: Idle, Running, or Suspended (JSPI, inside `hd:rt/block`).
  Only Idle starts an export. While Suspended, a completion resolves the
  Promise that the suspended `block` import waits on, and nothing else;
  `block` then writes the queued handles and returns. While Running or
  Suspended, completions only queue. When an export's Promise settles,
  the glue returns to Idle and, if the queue is non-empty, schedules the
  next wake-and-poll task. So no export re-enters a Wasm stack that JSPI
  suspended.
- **Cancellation** calls `AbortController.abort()` for fetches and
  `clearTimeout` for timers.
- **`block_on`** (answer 9):
  1. **JSPI present** (`WebAssembly.Suspending` exists): the glue wraps
     `hd:rt/block` as a suspending import, and wraps `hd.init`, `hd.poll`
     and the test exports with `WebAssembly.promising`. `block` then
     waits on a real Promise.
  2. **No JSPI, and the module imports `hd:rt/block`** (mine; Codex
     re-review N-B5): a restricted browser profile, scoped to `block`.
     std's `block_on` increments an exported mutable global,
     `hd.blocking`, around its loop. A `.start` that runs while it is
     non-zero is synchronous: a same-origin HTTP request is a
     synchronous `XMLHttpRequest` that finishes inside `.start`, and
     every other operation that would pend panics `host-contract` with
     a message that names JSPI. A `.start` outside `block_on` keeps the
     ordinary asynchronous path, cancellation included, so a retained
     but untaken `block_on` changes nothing elsewhere. The conformance
     runner tests each refused operation in this profile.
  3. **No `hd:rt/block` import**: the normal asynchronous mode, on every
     browser with Wasm GC.

### 17.7 Capability Grants

- **Startup refusal.** Before instantiation, the host reads the import
  list, maps each `hd:<Key>` module to its trait, and refuses with status
  101 when a needed trait is totally denied, naming the setting
  ([Total Deny](../../spec/cli/command-line.md#total-deny)). Nothing runs
  before the check, not even init.
- **Partial deny.** The generated host stub extracts the method's
  resource argument, checks it against the grant, and on a refusal
  encodes the method's `NotGranted` variant as the result. The program
  sees an ordinary `.Err`
  ([`cli.cap.partial.refuse`](../../spec/cli/command-line.md#r-cli.cap.partial.refuse)).
  `Env.get` returns `.None` and the host prints the one-line notice once
  per name.
- Checks live in host providers only, never in hd code, as
  HOST_CAPABILITIES requires.

### 17.8 Resource Limits

| Limit | Mechanism | Failure |
| --- | --- | --- |
| time (`--time-limit`, a test's `timeout`) | epoch interruption: one ticker thread per process increments the engine epoch every millisecond; each store sets its deadline in ticks | the trap maps to `time-limit`; a pending host wait is cut by a reactor timer at the same deadline |
| GC heap and the exchange buffer (`--max-heap`) | one aggregate budget per store, enforced by a `ResourceLimiter`. A refused linear-memory growth makes `memory.grow` return `-1`, which the writer turns into the `heap-exhausted` panic (§17.3). A GC allocation that fails after a collection traps; the trap maps to `heap-exhausted`. Slice 0a tests both on the pinned wasmtime | `heap-exhausted` |
| host-side buffers (a read result waiting for `.finish`, a reactor body) | a separate per-store byte budget in the host, checked before the host allocates; the default is the `--max-heap` value | the operation's result is the `heap-exhausted` panic at `.finish` |
| stack | `Config::max_wasm_stack`, part of the runtime profile | `stack-exhausted` |
| instances, tables, memories, GC heaps | the pooling allocator's totals (§18.3). The runner never runs more stores at once than the pool has slots, so exhaustion means a bug | an internal error naming the limit |

The flag names are open question 4; answer 11 settled the categories but
not the flags.

### 17.9 The Embedding API

The CLI is a host on this API (a Day 1 decision); the test runner and
later embedders are others.

```rust
pub trait Engine: Send + Sync {                       // wasmtime natively; the JS host in the browser
    type Module: Send + Sync;
    fn load(&self, wasm: &[u8], cache: &dyn CacheStore) -> Result<Self::Module, LoadError>;
    fn instantiate(&self, m: &Self::Module, host: &HostSetup) -> Result<Box<dyn Instance>, StartError>;
}
pub trait Instance {
    fn init(&mut self) -> Outcome;                    // runs hd.init
    fn poll(&mut self) -> Poll<Outcome>;              // runs hd.poll
    fn wake(&mut self, handles: &[u32]);
    fn call_test(&mut self, export: u32) -> Poll<Outcome>;
}
pub trait Provider: Send + Sync {
    fn key(&self) -> HostKey;
    fn call(&self, m: MethodId, args: &[u8], out: &mut Vec<u8>, cx: &mut CallCx) -> CallResult;  // Ready | Pending(OpId)
}
pub struct HostSetup {
    pub providers: ProviderSet,    // one per bound trait
    pub grants: Grants,            // from hd.toml and flags, or the test grant
    pub limits: Limits,
    pub reactor: ReactorHandle,
}
pub enum Outcome { Exit(u8), Panic(PanicReport), Internal(String) }
```

- `hd run` builds a `HostSetup` from the default profile's providers and
  the package's grant. `hd test` builds one per test kind (§19.3).
- A later embedder supplies its own providers for its own capability
  traits through the same `Provider` trait. Their ABI descriptions then
  come at run time instead of from the static table. That is the Later
  embedding feature, and the API leaves room for it.

### 17.10 The Generated JS Glue

One ES module, `hd-host.js`, generated from `hd_host_abi` and checked in:

- `makeImports(module, providers, grants)` returns the import object for
  exactly the module's imports;
- the codecs of §17.4, reading and writing the exchange buffer through a
  `DataView`, and strings through `TextDecoder` and `TextEncoder`;
- the poll and wake loop of §17.6, the JSPI detection, and synchronous
  mode;
- panic decoding: reads the panic globals and the buffer, maps
  `wasm-function[i]:0xOFF` frames through `hd.lines`;
- a provider interface; the playground supplies browser providers (§20.6).

It uses only web platform APIs, so it also runs under Node, which the
metric scripts can use to run a release `.wasm` on V8.
