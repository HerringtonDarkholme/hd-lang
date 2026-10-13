# New Compiler: The Playground on Wasm

Part of the [compiler design](README.md).

Status: plan, 2026-10-12, issue #143 step 1. The owner retires the TS
compiler (`src/`). This note plans how the website playground compiles
and runs hd with the Rust compiler in the browser. The design is in
[design-overview.md §2.1-§2.2](design-overview.md),
[scheduler.md §6.2](scheduler.md) (stepping) and
[cache.md §5.8](cache.md) (browser store). The numbers here were measured
on `16553a7a` (Apple Silicon, `wasm32-unknown-unknown` installed). They
are measurements unless marked as estimates.

## 1. What Builds For wasm32 Today

`cargo check --target wasm32-unknown-unknown` passes for every crate on
the browser path. That covers `hd_base`, `hd_intern`, `hd_diag`,
`hd_syntax`, `hd_fmt`, `hd_project`, `hd_types`, `hd_resolve`, `hd_tir`,
`hd_structure`, `hd_check`, `hd_mono`, `hd_host_abi`, `hd_wasm`,
`hd_cache`, `hd_sched`, `hd_driver`, `hd_run`, `hd_stdpack` and `hd_web`.
A type check is not enough on this target: several `std` calls compile
but fail at run time. Here is what actually reaches them:

| Hazard | On wasm32-unknown-unknown | Where on the browser path | Action |
|---|---|---|---|
| `rayon` threads | builds; spawning threads fails | `hd_driver` forces `hd_sched/threads`; `run_pool` falls back to serial | size only: feature split (below) |
| `std::fs`, `std::process` | always `Err` | `hd_cache::DiskStore` (behind `disk`), `hd_run::grant` (`canonicalize`; the rest is tests) | none: not reached; drop `hd_run` from `hd_web` |
| `Instant::now()` | **panics** | none in library code (`hd_mono` layout uses it only in tests); `hd_driver` takes a `Clock`, and `NoClock` exists | keep it out; the smoke test catches it |
| `env::args()` | always empty | examples only | none |
| `thread::scope` | panics | tests only (`hd_base`, `hd_types`) | none |
| panics | abort (no unwinding) | no library `catch_unwind` | an internal error kills the instance; the worker restarts it |
| wasmtime | not built | `hd_run_wasmtime` is not a dependency | none |

So **nothing blocks the build**. The work is a feature split plus an
export layer:

1. `hd_driver` gets a `threads` feature, on by default, that enables
   `hd_sched/threads` and gates `Executor::Pool`. `hd_cli` turns it on.
   `hd_web` depends on `hd_driver` with `default-features = false`.
   Rayon and crossbeam then leave the wasm tree. Rule 2.2.1 already
   requires this ("rayon only behind `hd_sched`'s `threads` feature"),
   but the driver currently enables the feature unconditionally.
2. `hd_web` drops its unused `hd_run` dependency.
3. CI builds `hd_web`, not only `hd_syntax`, for wasm32. That is rule
   2.2.5; `test.yml` builds only `-p hd_syntax` today.
4. LTO with abort-on-panic deletes everything after a panicking call.
   Two examples showed it: `bench`, which calls `Instant::now()`, and
   `tir`, which exits early on empty `args()`. Each shrank to 15 KB. So
   an `Instant` that slips into the browser path produces a tiny wasm
   rather than a build error. The Node smoke test (W3) compiles a real
   program, and the size gate catches the shrink.

## 2. How Std Reaches the Compiler

std is **already embedded**. `hd_driver/build.rs` `include_str!`s all 38
`lib/std` files into `STD_FILES`, and `std_sources()` adds the virtual
`core.hd` and `rt.hd`. The browser build inherits this unchanged.

| Option | Bytes (raw / gzip) | Cold compile | Status |
|---|---|---|---|
| Embedded source (today) | 507 KB / 115 KB inside the compiler wasm | parses and builds the interfaces of the std folders it reaches | works now |
| Prebuilt std pack (interface blobs) | unknown, likely larger than gzipped source | skips std parsing and interface building | `hd_stdpack::build_pack` still refuses ("std pack writer") |
| Separate fetched asset | same as embedded | same, plus a request | no gain: every compile needs std |

Recommendation: keep the embedded source. The worker keeps one
`MemoryStore` alive, so only its first compile pays for std. Revisit
when the std pack writer exists.

## 3. The JS API and the Host

### 3.1 Compiler API (`hd_web`)

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
  tests?: TestCase[];             // goal "test": hd_driver::TestCase (program, name, file, line, kind, ignore, expectPanic, unsupported, seed, run: [test, init]), in hd test's report order
}
```

- `wat` compiles as `run` (goal byte 3), then `hd_web` prints the
  program with `wasmprinter` (no default features: no validation, no
  component model). The text is output kind 2, read with the existing
  `hd_output(2)` and `hd_output_len(2)`; it is empty for every other
  goal. Names come from the name section `hd_wasm` writes.
- `run` is `Goal::Program { entry }`. A file without `main` compiles as
  a script, `Roots::Script`, whose top-level statements run as its init.
  `check` is `Goal::Analyze`, which is `hd check`. `test` builds the
  programs `hd test` builds (`hd_driver::test_programs`): the unit-test
  program, then each integration test file as its own program, each a
  `Goal::Tests` build.
- `hd_project::Root::of` already maps `src/`, `tests/` and `tasks/`
  paths, so playground paths pass through unchanged. `hd_web` turns the
  entry file into its module path.
- Diagnostics reuse `hd_cli::report::from_output`, moved into
  `hd_driver` so the CLI and the web share one shape. The move adds the
  end position: `Output::locate` of the span's `hi`.
- The session keeps `sources` and `store` across calls. `MemoryStore`
  never evicts, and its `new` list grows until something drains it. The
  session therefore drains after each call, and drops the store once its
  entries pass 64 MB. The next compile is then cold, costing about
  200 ms.

**Wire format.** Requests are LEB128 length-prefixed UTF-8: the file
count, then path and text pairs, then the entry, then a goal byte. A
response is JSON text, written by hand as `hd_cli`'s `quote` does
(rule 2.2.4: no serde), plus the wasm bytes in a second buffer. The
exports are `hd_input(len) -> ptr`, `hd_compile() -> status`,
`hd_output(kind) -> ptr` and `hd_output_len(kind) -> len`. They work over
buffers the crate owns, so Rust needs no raw-pointer `unsafe` blocks.
The only unsafe is the `#[unsafe(no_mangle)]` attribute, which the owner
approved (Q1).

**REPL goals** (repl-plan.md job W3, #17). Three more goal bytes drive
one REPL session per worker, `hd_web::Session::repl_start`,
`repl_input` and `repl_outcome`. Each request builds or settles one
module of the session; JS links and runs the modules (W4).

| Goal byte | Request after the goal byte | Response JSON | Output kind 1 |
| --- | --- | --- | --- |
| 4 repl start | nothing; the files are the session's package, entry empty | `{"diagnostics","input":0,"sessionKeys"}` | the prelude |
| 5 repl input | the input's text; no files, entry empty | `{"diagnostics","input":N,"sessionKeys"}` | input N's module, when it built |
| 6 repl outcome | module number, then outcome byte (0 joined, 1 dropped); no files, entry empty | `{"diagnostics":[],"input":N,"joined"}` | empty |

- Repl start drops any earlier session and builds the prelude
  (`build_repl_prelude`). The files act as the package: the session
  sees `src/lib.hd` (`cli.repl.package.lib`). A broken package file
  fails every input, as on the CLI.
- Repl input builds `repl#N` with `Goal::ReplInput` over the inputs
  that joined; diagnostics name `repl#N`. An input that does not build
  is dropped at once (status 1, no bytes); its number stays used.
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
- The value line needs no field: the module writes it to stdout
  (`cli.repl.value.stream`). Redeclaration is an error
  (`cli.repl.redeclare.*`), so there is no stale-caller note.

**Worker.** `worker.ts` keeps its message protocol (`project`, `wat`,
`stdout`, `result`). `runner.ts` is replaced by a `rust-runner.ts` that
returns the same `RunResult` and `RunDiagnostic`. `main.ts`,
`output.ts`, `editor.ts`, `client/tour.ts` and the `CompilerClient`
therefore do not change. The `repl` and `repl-restore` requests answer
"not available" until #17.

### 3.2 Running the Output

The existing browser host does **not** run the Rust compiler's output.
The ABIs differ:

- The TS emitter imports one `"hd"` module and answers structured
  `HostSuspensionCall` values (`src/web-host.ts`,
  `playground-host.ts`).
- The Rust output follows `hd_host_abi`. Each trait is its own `hd:<Trait>`
  import module, with `.start` and `.finish` pairs for methods that may
  wait. Arguments cross the `hd.x` exchange buffer. The exports are
  `hd.init`, `hd.poll` and `hd.wake`, plus `hd.test.i` and `hd.init.j`
  for tests.
- `host/core.mjs` speaks this ABI, but it imports `node:fs`,
  `node:child_process`, `node:net`, `node:worker_threads`, `node:crypto`,
  `node:os` and `node:path` at the top. It uses `Buffer`, and
  `hd:rt.block` uses `Atomics.wait` on a `SharedArrayBuffer`. A browser
  provides `SharedArrayBuffer` only on cross-origin-isolated pages, which
  GitHub Pages cannot serve.
- `hd_host_abi::generate_js_glue` is a stub ("JS glue generator").

**Split, do not fork.** Split `host/core.mjs` into a platform-neutral
core and a Node layer:

- The neutral core holds `Enc`/`Dec`, the grant, and the `rt`, `prim`,
  `Console`, `Clock`, `Random`, `Args`, `TestRunner` and
  `PropertyRunner` providers, plus the `run` driver. It uses
  `TextEncoder`/`TextDecoder` and `crypto.getRandomValues`.
- `block` keeps `Atomics.wait` when `SharedArrayBuffer` exists. Otherwise
  it spins until the deadline, as `src/web-host.ts`'s `wait` does today.
- The Node layer adds `Fs*`, `Process`, `Net`, `Http`, `Env`, `Sys` and
  `ConsoleInput`.
- `hd_cli/src/node.rs` embeds one more file.
- A browser entry, `host/web.mjs`, adds the playground's `Http`. That is
  `sendFromBrowser`, a synchronous XHR limited to the page's own origin,
  re-encoded with `Enc`/`Dec`.

Before it instantiates a program, the runner reads
`WebAssembly.Module.imports`. A `hd:<Trait>` module the playground does
not bind is refused with today's text: "main needs FsRead, which the
playground does not provide". This mirrors `hd_run::check_grants`.

The output targets Wasm GC, so the browser floor stays at Chrome 119,
Firefox 120 and Safari 18.2. The compiler wasm needs only the features
Rust enables by default for wasm32, such as bulk memory and sign
extension. All of them predate Wasm GC.

## 4. Cost Model

| Stage | Measured | Browser (estimate) | Notes |
|---|---|---|---|
| Compiler wasm, release, no LTO, full `Goal::Program` path, custom sections stripped | 4.97 MB raw, 1.56 MB gzip | | code 4.19 MB, data 0.78 MB (std text 0.51 MB), name section 0.75 MB |
| Same with `lto`, `codegen-units = 1`, `opt-level = "s"`, `panic = "abort"`, `strip` (the `stages` example; the emitter is present) | 3.21 MB raw, 1.15 MB gzip | | LTO build about 14 s with dependencies warm |
| Then `wasm-opt -all -Oz` (binaryen 132, already in `node_modules`) | **2.75 MB raw, 1.12 MB gzip** | about 0.95 MB brotli | today's worker is 14.6 MB, mostly binaryen.js |
| W3's `hd_web` cdylib (`--profile web`, the four exports, std embedded) | **3.86 MB raw, 1.24 MB gzip**; `wasm-opt -all -Oz` gives 3.25 MB raw but 1.25 MB gzip and takes 90-120 s, so the build skips it | | the two rows above come from `stages`, which reads std from disk, so they lack std's 0.51 MB of text; W3's gate is ≤ 4 MB raw, ≤ 1.3 MB gzip, ≥ 2 MB raw |
| Download plus streaming compile | | about 1.1 MB on the wire; Liftoff compiles 2.75 MB in tens of ms | overlaps download; fetched once per page, from the HTTP cache after that |
| Cold compile of a playground example (std reached, serial, in-memory store) | 70-80 ms native release (`inventory.hd`, `derive.hd`); bench cold 97 ms | 120-250 ms desktop; up to about 600 ms on phones | Wasm runs about 1.5-2.5x native |
| Warm compile (same session, store kept) | 6.5 ms with no edit, 12 ms after a body edit (bench) | 10-30 ms | this is Run after Check, or an edit |
| Run: instantiate a 5-9 KB program | | under 5 ms | output from the examples measured above: 5.5 KB and 8.7 KB |
| Memory | 33-34 MB peak RSS for the whole native process | 20-40 MB linear memory after a cold compile; never shrinks | the 64 MB store cap bounds edit growth; Stop or a timeout frees everything by restarting the worker |

The figures above put the stages' sizes in order:

- **Granularity:** one compile is one call. A single module takes about
  200 ms, which the 15 s stop and timeout cover, so the stepping
  scheduler (scheduler.md §6.2) can wait.
- **Incremental work:** comes from the kept `MemoryStore`.
- **I/O:** IndexedDB persistence (cache.md §5.8) would save only the
  cold compile after a page reload, so it is a later job.
- **Restart:** a Stop terminates the worker. The reload then costs a
  compile from the HTTP cache plus one cold compile, about 300 ms. That
  is cheap enough to keep one worker, as today.

## 5. What the Playground Loses Meanwhile

1. **The REPL.** The website's REPL panel and its **Try in REPL** buttons
   go, and so does Run's REPL semantics for a file without `main`
   (`42 : i32` echoes). The Rust REPL is #17. Until then:
   - a file without `main` runs as a script, so its top-level statements
     run and print only what they print;
   - a declarations-only file runs nothing, and **Test** runs its tests;
   - Check checks the whole file, not input by input;
   - **Try in REPL** turns into the existing **Open in playground** link.
2. **Panic source lines.** Diagnostics keep their positions, but a
   runtime panic's frames need `Output::positions`. That is a later
   `locate(frames)` call on the session.
3. **Per-case test `timeout` for busy loops.** `test.mjs` supervises
   cases from a worker thread. In the browser, a body that never yields
   is cut only by the 15 s run limit. Waits are still cut at the
   deadline. Snapshot and regression files live in memory for one run.
4. **Highlighting does not change.** `src/highlight.ts` and its
   `KEYWORDS` (from `src/lexer.ts`) move into `website/`. A test checks
   the keyword list against `hd_syntax`.
5. **Website build-time uses of `src/` change.** `website/src/markdown.ts`
   calls the TS `parse()` to decide which blocks get a button. It can
   call the compiler wasm in Node instead, or drop the check. The REPL
   button is gone anyway (item 1). `learn-check.ts` (via
   `test/hd-in-process.ts`) belongs to #144/#145, not to this note.
6. **Formatting.** The playground has no Format button today.
   `hd_fmt` builds for wasm32, so one is cheap later. It is not part of
   this plan.
7. **Behaviour differences.** Where the Rust compiler and the TS
   prototype disagree, the playground now follows the Rust compiler.
   `test/runner.test.ts` expectations move with it.

## 6. Owner Decisions

Decided 2026-10-12:

- **Q1. Exports use `#[unsafe(no_mangle)]`.** Four hand-written exports
  in one `exports` module of `hd_web`, with `#[allow(unsafe_code)]` on
  that module only and no unsafe blocks. No wasm-bindgen. This is the
  second approved unsafe, after the `libc::_exit` emergency exit.
- **Q2. The WAT tab stays; the REPL panel goes until #17.** The WAT view
  prints the module with `wasmprinter` (job W8, before W7), an estimated
  +0.3 MB raw.

## 7. Step Plan

Each job is at most about 1 hour of agent work. Each one ends green on
the tiered checks and with its own done check.

| # | Job | Done check |
|---|---|---|
| W1 | `hd_driver` gets a `threads` feature (default on) gating `Executor::Pool`; `hd_cli` enables it. `hd_web` uses `default-features = false` and drops `hd_run`. CI builds `-p hd_web` for wasm32. | `cargo tree -p hd_web --target wasm32-unknown-unknown` lists no rayon or crossbeam. `cargo check -p hd_web --target wasm32-unknown-unknown` passes. Native `cargo test`, clippy and conformance stay green. The CLI footprint (tiny-program functions and build time) is unchanged. |
| W2 | `hd_web` compile API in plain Rust: `compile(files, entry, goal)`, the store drain, the 64 MB cap. Move `from_output` into `hd_driver`, add end positions, and switch `hd_cli` to it. | `hd_web` unit tests: a playground example yields wasm; a broken file yields diagnostics with the same code, line and column as `hd check --format json`; a `tests:` file lists its cases. The CLI's JSON output is unchanged. |
| W3 | The wasm exports (Q1), crate-type `cdylib`, a `[profile.web]` (release plus LTO, cu 1, opt "s", abort, strip), and a script that builds into the playground's assets (no `wasm-opt`: it made gzip larger). | A Node test loads the wasm, compiles `println-console-stdout.hd` and runs the bytes with `host/run.mjs`, which prints the expected line. The size gate (≤ 4 MB raw, ≤ 1.3 MB gzip, ≥ 2 MB raw) also catches an LTO shrink. |
| W4 | Split `host/core.mjs` into a neutral core plus a Node layer, with the spin fallback in `block`. `node.rs` embeds the new file. | `grep "node:"` finds nothing in the neutral core. `cli_conformance` on the Node engine is green. `hd test` on a std test file is unchanged. |
| W5 | `rust-runner.ts` for Check and Run: compile in the worker, refuse unbound capabilities from the module's imports, run with `host/web.mjs` (Console to `stdout` messages, Clock, Random, Http, Args), report status and panics as `RunResult`. | `runner.test.ts` runs every example on Node with the real wasm and the expected output and exit status. `website:e2e` Run and Check steps pass. |
| W6 | Test mode: the case loop of `test.mjs` (fresh instance per case, `it_each` rows, the property runner and shrinking) moves into a neutral module that `test.mjs` and the browser share. | Test on `examples/tests.hd` gives the same pass and fail per case as `hd test` on that file. The Node `hd test` path is unchanged. |
| W7 | Cut `src/` from the website: move highlighting, check paths locally in `project.ts`, hide the REPL panel (the WAT tab stays, fed by W8), gate Markdown buttons via the wasm in Node or drop the gate, remove the binaryen worker bundle, rewrite `website/playground/README.md`. `pages.yml` installs the wasm32 target and runs the W3 build before the website build. | No website file imports the TS compiler: `grep -rnE 'src/(highlight|lexer|parser|repl|compiler|package|web-host|test-runner|runtime-panic|diagnostics|hir|emitter|checker)' website --exclude-dir=node_modules --exclude-dir=dist` finds nothing. `website:build` and `website:e2e` pass. The worker bundle size is reported. |
| W8 | WAT view: `hd_web` prints the compiled module with `wasmprinter`; the worker's `wat` request returns that text. Runs before W7. | The WAT tab shows the module for every playground example. The W3 size gate is raised by the measured `wasmprinter` cost, reported in the commit. |

**Later.** In rough order:

1. panic positions through `locate`;
2. IndexedDB persistence of store entries (cache.md §5.8);
3. stepping slices with cancel on edit (scheduler.md §6.2);
4. the REPL tab, after #17.
