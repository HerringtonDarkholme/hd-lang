# New Compiler: The Playground on Wasm

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

How the website playground compiles and runs hd with the Rust compiler
in the browser. The general design is in
[docs/compiler/design-overview.md §2.1-§2.2](../../docs/compiler/design-overview.md),
[crates/hd_sched/DESIGN.md §6.2](../hd_sched/DESIGN.md) (stepping) and
[crates/hd_cache/DESIGN.md §5.8](../hd_cache/DESIGN.md) (browser store).

## 1. Building For wasm32

`hd_web` is a `cdylib` (`--profile web`: release plus LTO, one codegen
unit, `opt-level = "s"`, `panic = "abort"`, `strip`), linked with
`--compress-relocations` by `scripts/build-web-compiler.js`. It depends on
`hd_driver` with `default-features = false`, so the driver's `threads`
feature (which enables `hd_sched/threads` and `Executor::Pool`) and
rayon stay out of the wasm tree; `hd_cli` turns the feature on.

A type check is not enough on `wasm32-unknown-unknown`: several `std`
calls compile but fail at run time.

| Hazard | On wasm32-unknown-unknown | How the browser path avoids it |
|---|---|---|
| `rayon` threads | builds; spawning threads fails | not a dependency (above) |
| `std::fs`, `std::process` | always `Err` | `hd_cache::DiskStore` is behind the `disk` feature; `hd_web` does not depend on `hd_run` |
| `Instant::now()` | **panics** | `hd_driver` takes a `Clock`; `hd_web` passes `NoClock` |
| `thread::scope` | panics | tests only |
| panics | abort (no unwinding) | an internal error kills the instance; the page restarts the worker |
| wasmtime | not built | `hd_run_wasmtime` is not a dependency |

LTO with abort-on-panic deletes everything after a panicking call, so an
`Instant::now()` that slips into the browser path produces a tiny wasm
rather than a build error. The size gate in
`scripts/test-web-compiler.js` has a floor as well as a ceiling to catch
that shrink.

## 2. How Std Reaches the Compiler

std is embedded: `hd_driver/build.rs` `include_str!`s every `lib/std`
file into `STD_FILES`, and `std_sources()` adds the virtual `core.hd` and
`rt.hd`. The browser build uses the same sources. The worker keeps one
`MemoryStore` alive, so only its first compile parses std and builds the
interfaces of the std folders a program reaches. A separately fetched
std asset would gain nothing, since every compile needs std.

Planned, not built: a prebuilt std pack of interface blobs, which would
skip std parsing; `hd_stdpack::build_pack` still refuses ("std pack
writer").

## 3. The JS API and the Host

### 3.1 Compiler API (`hd_web`)

A `Session` is one worker's state: the playground's files, one
`MemoryStore` kept across compiles, and at most one REPL session. Each
compile is one serial driver run (`Executor::Serial`) with `NoClock`, on
the browser pipeline (`EngineTarget::Browser`, `OptLevel::Dev`).

```ts
type Goal = "run" | "check" | "test" | "wat" | "replStart" | "replInput" | "replOutcome";
interface CompileRequest {
  files: Record<string, string>;  // "src/main.hd" -> text (playground paths as they are)
  entry: string;                  // "src/main.hd"
  goal: Goal;
}
interface CompileResult {
  wat?: string;                   // goal "wat": the program as WebAssembly text (wasmprinter); output kind 2
  wasm?: Uint8Array;              // run, wat and test, when there are no errors; test: the programs one after another
  diagnostics: RunDiagnostic[];   // path, code, severity, message, line, column, endLine, endColumn
  programs?: { file: string | null; size: number }[];  // goal "test": the unit-test program, then each tests/ file's
  tests?: TestCase[];             // goal "test": hd_driver::TestCase, in hd test's report order
}
```

- `run` is `Goal::Program { entry }`, over the library, the executables
  and the entry, as `hd run FILE` narrows a package. A file without
  `main` compiles as a script, whose top-level statements run as its
  init. One file outside the `src` layout is a single-file program.
  `check` is `Goal::Analyze`, which is `hd check`. `test` builds the
  programs `hd test` builds (`hd_driver::test_programs`): the unit-test
  program, then each integration test file as its own `Goal::Tests`
  build.
- `wat` compiles as `run`, then prints the program with `wasmprinter`
  (no default features: no validation, no component model). Names come
  from the name section `hd_wasm` writes.
- `hd_project::Root::of` maps `src/`, `tests/` and `tasks/` paths, so
  playground paths pass through unchanged.
- Diagnostics use `hd_driver::diagnostics::from_output`, the shape the
  CLI's JSON output uses, with end positions.
- `MemoryStore` never evicts, and no host writes its entries back yet.
  So the session drains the new-entry list after each call and drops the
  store once its entries pass 64 MiB (`STORE_CAP_BYTES`); the next
  compile is then cold.

**Wire format** (`hd_web::wire`, which documents each goal's fields and
answers). Requests are LEB128 length-prefixed UTF-8: the file count,
then path and text pairs, then the entry, then a goal byte (0 run,
1 check, 2 test, 3 wat, 4 repl start, 5 repl input, 6 repl outcome),
then the goal's own fields. A response is JSON text, written by hand
(rule 2.2.4: no serde), plus the program bytes in a second buffer. The
exports are `hd_input(len) -> ptr`, `hd_compile() -> status`,
`hd_output(kind) -> ptr` and `hd_output_len(kind) -> len`. They work over
buffers the crate owns, so Rust needs no raw-pointer `unsafe` blocks
(§6).

**REPL goals** drive one REPL session per worker,
`Session::repl_start`, `repl_input` and `repl_outcome`. Each request
builds or settles one module of the session; JS links and runs the
modules.

| Goal byte | Request after the goal byte | Response JSON | Output kind 1 |
| --- | --- | --- | --- |
| 4 repl start | nothing; the files are the session's package, entry empty | `{"diagnostics","input":0,"sessionKeys"}` | the prelude |
| 5 repl input | the input's text, then optionally its number N; no files, entry empty | `{"diagnostics","input":N,"sessionKeys"}` | input N's module, when it built |
| 6 repl outcome | module number, then outcome byte (0 joined, 1 dropped); no files, entry empty | `{"diagnostics":[],"input":N,"joined"}` | empty |

- Repl start drops any earlier session and builds the prelude
  (`build_repl_prelude`). The files act as the package: the session
  sees `src/lib.hd` (`cli.repl.package.lib`). A broken package file
  fails every input, as on the CLI.
- Repl input builds `repl#N` with `Goal::ReplInput` over the inputs
  that joined; diagnostics name `repl#N`. An input that does not build
  is dropped at once (status 1, no bytes); its number stays used. N is
  the request's number when it gives one, which must be greater than
  every earlier module's, else one more than the last. So a session
  rebuilt in a fresh worker builds its kept inputs again under their old
  numbers, and the diagnostics, `dbg` lines and journal keep the numbers
  the page shows.
- `sessionKeys` are the module's exports that
  `hd_wasm::is_session_key` accepts. JS registers them under `hd.s` for
  later modules. The entry exports are not among them.
- **Outcomes travel as their own request.** A module that built waits
  until JS reports how it ended. Outcome byte 0 means status 0: the
  module joins (`hd_driver::Session::add`), and later inputs see its
  names. Byte 1 covers a failed instantiation, a panic, a stop or
  cancel, and a nonzero status. The input then adds nothing and its
  file leaves the source set (`cli.repl.panic.binding`). A prelude
  that does not join ends the session.
- While a module waits, another repl input is refused. So is an
  outcome for another module, or any REPL request before a start.
  These answer status 2 with `{"error"}`, as a malformed request does.

**Worker.** `website/playground/src/worker.ts` runs the compiler wasm;
`rust-runner.ts` turns a compile into a `RunResult` with
`RunDiagnostic`s; `compiler-wire.ts` encodes requests; the `repl-*.ts`
files drive the REPL panel.

### 3.2 Running the Output

The output follows `hd_host_abi` (crates/hd_run_wasmtime/DESIGN.md §17):
each trait is its own `hd:<Trait>` import module, with `.start` and
`.finish` pairs for methods that may wait; arguments cross the `hd.x`
exchange buffer; the exports are `hd.init`, `hd.poll` and `hd.wake`, plus
`hd.test.i` and `hd.init.j` for tests.

The JS host is split, not forked:

- `host/core.mjs`, the platform-neutral core, holds the import object
  built from the generated codecs and method descriptors
  (`host/gen/abi.mjs` over `host/wire.mjs`'s `Enc`/`Dec`), the grant,
  the `rt`, `prim`, `Console`, `Clock`, `Random`, `Args`, `TestRunner`
  and `PropertyRunner` providers, and the entry driver. It
  uses only web-standard APIs. Its synchronous sleep uses `Atomics.wait`
  where `SharedArrayBuffer` exists and spins otherwise, since GitHub
  Pages cannot serve the cross-origin-isolation headers
  `SharedArrayBuffer` needs.
- `host/node.mjs` adds `Fs*`, `Process`, `Net`, `Http`, `Env`, `Sys` and
  `ConsoleInput`; `hd_cli/src/node.rs` embeds it.
- `host/web.mjs` adds the playground's `Http`, a synchronous
  `XMLHttpRequest` limited to the page's own origin, and
  `TestRunner.temp_dir`. The program runs in a dedicated worker, which
  still allows a synchronous request.
- `host/cases.mjs` holds the test case loop (a fresh instance per case
  and per row, the property runner and shrinking), shared by
  `host/test.mjs` and the playground.

Before it instantiates a program, the runner reads
`WebAssembly.Module.imports` (`unboundTraits`). A `hd:<Trait>` module the
playground does not bind is refused: "main needs FsRead, which the
playground does not provide". This mirrors `hd_run::check_grants`.

The output targets Wasm GC, so the browser floor is Chrome 119,
Firefox 120 and Safari 18.2. The compiler wasm needs only the features
Rust enables by default for wasm32, such as bulk memory and sign
extension. All of them predate Wasm GC.

## 4. Cost Model

- **Size.** The compiler wasm is gated by
  `scripts/test-web-compiler.js` (raw and gzip ceilings and a raw floor);
  that script records each raise and its cause. The build keeps compile
  speed over download: `opt-level = "z"` would make the gzip download
  167 KB smaller but compiles about 25% slower, so the profile stays at
  `"s"`. `wasm-opt -Oz` is not run:
  it made gzip larger and took over a minute.
- **Granularity:** one compile is one call. A playground program
  compiles in a few hundred ms cold, which the page's 15 s run limit
  covers, so the stepping scheduler (crates/hd_sched/DESIGN.md §6.2) can
  wait.
- **Incremental work** comes from the kept `MemoryStore`: Run after
  Check, or an edit, reuses every entry that did not change.
- **I/O:** IndexedDB persistence (crates/hd_cache/DESIGN.md §5.8) would
  save only the cold compile after a page reload, so it is a later job.
- **Restart:** a Stop or a request over 15 s terminates the worker
  (`compiler-client.ts`). The fresh worker costs a compile from the HTTP
  cache plus one cold compile, cheap enough to keep one worker.

## 5. What the Playground Lacks

1. **Script semantics.** A file without `main` runs as a script: its
   top-level statements run and print only what they print; a bare
   expression echoes nothing (unlike the REPL panel).
2. **Panic source lines.** Diagnostics keep their positions, but a
   runtime panic's report has no source position: its frames need
   `Output::positions`, which the session does not expose yet.
3. **Per-case test `timeout` for busy loops.** A case past its `timeout`
   while it waits is cut at its deadline; a body that never yields is
   cut only by the 15 s run limit. Snapshot and regression files live in
   memory for one run (`memoryFiles`).
4. **Formatting.** The playground has no Format button. `hd_fmt` builds
   for wasm32, so one is cheap later.

## 6. Owner Decisions

- **Q1. Exports use `#[unsafe(no_mangle)]`.** Four hand-written exports
  in one `exports` module of `hd_web`, with `#[allow(unsafe_code)]` on
  that module only and no unsafe blocks. No wasm-bindgen.

## 7. Later

In rough order, none built:

1. panic positions through a `locate(frames)` call on the session;
2. IndexedDB persistence of store entries (crates/hd_cache/DESIGN.md §5.8);
3. stepping slices with cancel on edit (crates/hd_sched/DESIGN.md §6.2);
4. a threaded browser build (#189).
