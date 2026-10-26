# New Compiler Design: Command Flows

Status: design record; may lag the code. The code and the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

## 7. Command Flows

### 7.1 `hd check` (Package Mode)

1. Find the package or workspace ([Package Mode](../../spec/cli/command-line.md#package-mode)).
   Parse `hd.toml`, select dependency versions, verify `hd.sum`, and
   fetch what is missing ([Fetching](../../spec/cli/command-line.md#fetching)).
   A manifest error is reported here and stops the run, before any cache
   lookup. `hd --version` and `hd help` return before any of this.
2. Open the session: `toolchain_key` and the disk store. The thread pool
   is used when the thread count is above 1, and `hd check` creates no
   Wasm engine.
3. Read and skim every source file; build the folder graph from the
   skims' use lists.
4. Bottom-up over folders: read the folder's read list, compute
   `iface_key` from it, look it up, and on a miss parse the folder's
   files and run `FolderIface`.
5. For each module, compute `check_key`; on a hit take its result, on a
   miss parse it and run M1 to M3.
6. Run `HeaderCheck` per folder, `Coherence` once and `InitOrder` per
   folder, each looked up by its key first.
7. Print diagnostics in content order once every task that can add one
   is done (§6.5), then the summary. Text output ends with
   `check result: ok|FAILED. errors: E; warnings: W`. `--summary`
   prints one line per severity, file and code instead, in path then
   code order. `--max-errors N` stops printing after the Nth error;
   the counts and the exit status still cover everything. Eviction, if a
   shard is over budget, runs when the store is dropped (§5.7).

Touched: `iface`, `parse`, `check` and `graph` entries. Dependency
bodies are never parsed: interfaces come from syntax alone.

Planned, not built: the stat manifest diff, the `fast_key` lookup of a
stored package result, keys taken from a last-run record without opening
entries, and early cutoff (§5.3.1, §5.5); publishing on an I/O thread
after output (§5.4).

### 7.2 `hd check FILE`

1. As steps 1 to 4 above, but only for the folders that FILE's module
   deeply depends on, plus its own folder.
2. Check FILE's module and every module it uses, deeply: FILE's module
   "linked with the rest of the package" (`cli.package.file`). A module
   FILE does not reach stays silent, so a sibling's error is not
   reported (`cli/check-summary`), and a used module's is
   (`cli/json-file-location`). FILE's test code is checked too when FILE
   is test code.
3. Coherence runs only for traits with an impl in FILE.
4. Output: the diagnostics of those modules, in content order, plus one
   note per used folder whose interface has errors ("src/shop has 2
   errors; run hd check"). JSON output keeps its contract: each
   diagnostic, then the summary.
5. A FILE under no root is a single-file program
   ([`cli.package.no-root`](../../spec/cli/command-line.md#r-cli.package.no-root)):
   one module, std only, no manifest.

Implementation differs: the driver checks the bodies of FILE's module,
its doc tests and the modules they use, deeply (`Packages::check_roots`,
`Run::check_scope`), and builds the interfaces of their closures'
folders, all modules of each. Coherence runs over those interfaces, not
only for the traits with an impl in FILE. `check_cmd.rs` still filters
the output to the same modules, and there are no per-folder notes.

### 7.3 `hd test`

1. Check with test code: `tests:` blocks, test modules, integration test
   programs (each its own root over the package's public interfaces,
   [`module.test.integration.view`](../../spec/lang/10-modules.md#r-module.test.integration.view)),
   and doc tests.
2. On errors, print them and stop with status 101.
3. Build the **test plan** (`hd_run::tests_model::TestPlan`): the list of
   test programs and their cases. One unit-test program per package,
   holding every module's `tests:` cases and doc tests, with an init
   export per module (§19.1); one program per integration test file.
   Registration names are string literals
   ([Registration Calls](../../spec/lang/10-modules.md#registration-calls)),
   so the checker lists each program's cases without running anything.
   Only the row count of an `it_each` case is learned at run time.
   `--filter` is applied to this list.
4. Build and run the plan. The package's executables are built first
   ([`cli.test.builds-executables`](../../spec/cli/command-line.md#r-cli.test.builds-executables)).
   §20.1 continues this flow.

### 7.4 `hd test --affected`

Each **case group** has a record of the fingerprint of the last run in
which every case of that group ran and passed. A case group is one
module's unit cases and doc tests, or one integration test file. With
one unit-test program per package (§19.1), a program-wide fingerprint
would rerun every unit test after any edit, so the fingerprint is per
group:

```text
group_fp(g) = H("affected", toolchain_key, pipeline_hash, profile, test options,
                the TIR content hash of g's module (or g's file),
                sorted [(module path, tir_content_hash)] of the modules g's
                cases and init reach in the use graph)
```

That is the `prog_key` (§11.3) that a program of g's cases alone would
have. It covers, through the modules' content hashes (TIR, items, init
summary and registrations; crates/hd_cache/DESIGN.md §5.3), every
source change that can change what g's cases run. A comment edit
changes no content hash, so it selects nothing.

1. Check with test code as `hd test` does. This also computes every
   module's TIR content hash.
2. Select each case group whose fingerprint differs from its record or
   has no record. A group that failed, or ran only partly under a filter,
   has no record, so it is selected until it passes in full.
3. Build the package's unit-test program filtered to the selected
   groups' cases, as `--filter` does (§19.1), plus the selected
   integration programs, and run them. After the run, write a record for
   each group that ran in full and passed, and remove the record of each
   group that failed.

Removed modules need no old graph edges: the fingerprint of every group
that reached them changes.

Implementation differs (`affected.rs`): records live in
`build/test-records`, one file per program id, and a fingerprint hashes
the **source text** of the files the group reaches, not TIR content
hashes. It also hashes inputs the design left out: the `hd` binary and
engine, the environment variables, the manifests and dependency set, the
executables a test may start, and the data files a case may read
(`cli.test.fingerprint`). Not judged: those extra inputs make it sounder,
while source text makes a comment edit rerun the group's tests.

### 7.5 `hd run`, `hd build`

- **`hd run [NAME]`:** check the package without test code, then build
  the chosen executable's entry module from its reachable items and run
  it under the default profile.
- **`hd build`:** check the whole package. On errors, stop. Otherwise
  write `build/debug/NAME.wasm` per executable, or `build/release/` with
  `--release`.
- **A library-only package**: `hd build` checks it and writes only its
  cache entries. It writes no `.wasm` file.
- **A script**: an entry module with top-level statements and no `main`
  runs its module initialization as the whole entry behavior
  (`Roots::Script`, emitted by `hd_wasm::script_entry`).
- **A FILE with no `hd.toml` above it is a one-file program**
  ([`cli.file.run`](../../spec/cli/command-line.md#r-cli.file.run)):
  only FILE is its source, not its siblings.

§20.2 to §20.4 continue these flows.

### 7.6 `hd fmt`, `hd fix`

**`hd fmt`** (`fmt_cmd.rs`). Per file, one after another (`--jobs` is
accepted and ignored):

1. Full parse. A file with a syntax error is not formatted; its first
   syntax error is printed.
2. Lower the green tree to a Wadler-style document and print it within
   the line width (`hd_fmt::format`). Comments are trivia in gaps and are
   kept.
3. Write changed files. `--check` lists files that would change.

No type checking and no cache entries. Idempotence is tested on every
fixture (`fmt(fmt(x)) == fmt(x)`).

Planned, not built: formatting files in parallel, and skipping a file
whose source hash a manifest record marks as already formatted.

**`hd fix`** (`fix_cmd.rs`, `cli.fix.*`). Package mode only; it takes
no FILE, no `-p` and no `--format`, as the Commands table and
`cli.json.commands` leave them out.

1. Read every source file and the package manifest once. Checks run in memory: a
   tried state's texts reach the driver through `DiskSources::set_text`,
   and nothing is written until the end.
2. Check as `hd check` does with no flag (`check_cmd::check_package`).
3. Of each diagnostic, in output order, take its first `Exact` fix-it
   whose edits fit package source files, or a missing-capability fix
   that inserts grants into `hd.toml`. An overlapping edit waits
   (`cli.fix.overlap`). Other manifest fixes, dependencies and `std` are
   never edited. Capability fixes insert all missing grants together, so
   the round bound does not limit how many capabilities can be listed.
4. Apply the round's fix-its together and check again. The round is safe
   when no diagnostic key (code, severity, file, message) occurs more
   often than before; positions are not in the key, since edits move
   them. When it is not safe, try the fix-its one at a time in output
   order, each on top of those kept, so an added diagnostic is charged to
   the fix-it that brought it. A rejected fix-it is skipped in later
   rounds, which lets one it overlapped have its turn.
5. Repeat with the last check, at most 3 rounds (`cli.fix.rounds`).
6. Write each changed file through a temporary file and a rename, unless
   it changed on disk meanwhile. Print each change (`fixed: FILE:L:C:
   CODE: MESSAGE`), the diagnostics of the last check, and `fix result:`
   with the counts, all on standard error. Status 101 when an error
   remains (`cli.fix.status`).

Stage-A diagnostics are re-emitted from `iface` entries on a warm run, so
those entries keep the fix-its too, anchored like their diagnostics:
each edit is an anchor of its first token plus byte distances from it.

### 7.7 `hd doc`

- **Whole package** (`doc_cmd.rs`): check the package as `hd check`
  does; when that reports no error, write its pages, HTML and Markdown
  together, with `hd_doc`.
- **`hd doc NAME`:** check the package the same way, then print one item,
  module or standard library entry to standard output and write no file.

The design built interfaces only, with no bodies; `cli.doc.*` asks for
the full check, so the built flow is the right one.

### 7.8 `hd repl`

`hd` with no arguments on a terminal opens the REPL (`repl.rs`). It runs
on wasmtime only.

1. In package mode the session acts as code inside `src/lib.hd`
   ([`cli.repl.package.lib`](../../spec/cli/command-line.md#r-cli.repl.package.lib)):
   the driver adds `lib.hd`'s private items to each input's scope.
2. Each input is a synthetic file `repl#n` in folder `src`, built with
   `Goal::ReplInput`. Its scope is `lib.hd`'s scope, the uses of every
   earlier input, and the earlier inputs' top-level bindings with their
   fixed types
   ([`cli.repl.input-types`](../../spec/cli/command-line.md#r-cli.repl.input-types)).
3. A session thread with a guest-sized stack owns the engine's session
   (`WasmtimeSession`, or the Node session). The main thread reads
   inputs and, on Ctrl-C, stops an input that computes or cancels one
   that waits on the host (`cli.repl.stop`, `cli.repl.cancel`).
4. Each input builds a linked module importing the initialized storage
   and functions of earlier inputs. Only its numbered `hd.init.N` and
   `hd.poll.N` entries run live. A completed outcome commits the new
   compiler ABI. A panic keeps its mutations and code, with its declarations
   hidden from later inputs (`Packages::failed_inputs`); a stop or cancel
   commits neither code nor bindings, and the next input restores the last
   completed boundary.
5. `:reload` reads the package from disk and compiles a fresh linked base
   for the longest checking prefix. It replays recorded host results,
   drops the first divergent input and its suffix, and adopts a base for
   exactly the surviving inputs (`cli.repl.rebuild.prefix`).
6. When the active journal exceeds its threshold, the runtime snapshots
   completed memory/global state and archives the call rows. Ordinary
   continuation imports the initialized epochs without replay; historical
   results remain available for reload (`cli.repl.checkpoint.history`).

### 7.9 The Playground

```text
 page (main thread)          compiler worker (hd_web.wasm)    run worker (JS host)
 ──────────────────          ─────────────────────────────    ────────────────────
 editor ─ "check" / "run" ─► Session::compile: one serial driver run
                             ◄── diagnostics JSON, built Wasm
 "run" ──────────────────────────────────────────────────────► instantiate on V8
                                                                with host/web.mjs
 console, http ◄────────────────────────────────────────────── host calls
 "stop" / 15 s ──────────────────────────────────────────────► terminate the run
                                                                worker only
```

1. The compiler worker loads `hd_web`'s Wasm once; std is embedded
   (crates/hd_web/DESIGN.md).
2. Each request sends every file; the session keeps its `MemoryStore`, so
   unchanged entries are reused.
3. "Run" hands the built Wasm bytes to a separate program worker, which
   instantiates them with the JS host. Stopping or timing out a program
   terminates that worker only, so the compiler's warm state survives.
   The compiler worker answers Run and Test with what it built
   (`compileProject`, website/playground/src/rust-runner.ts), and the
   page's client hands that to `run-worker.ts`. A Stop or the run limit
   during a compile still restarts the compiler worker.

Planned, not built: filling the `MemoryStore` from IndexedDB when the
worker starts and draining new entries back after each run (§5.8);
stepping-scheduler checks that a newer edit cancels between slices
(§6.2).

### 7.10 Build Profiles: What Release Changes

Every rule in `spec/` that differs by build mode already says what
release does, so the compiler needs no spec decision. The full table:

| Rule | Debug / test | Release |
| --- | --- | --- |
| `types.arith.checked` / `types.arith.release` | `+`, `-`, `*`, `**`, unary `-` overflow panics (`integer-overflow`) | wraps two's complement |
| `types.arith.failure`, `types.arith.shift-count.debug` / `shift-count.release` | invalid shift panics (`invalid-shift`) | count masked to the value width |
| `types.arith.always` (division by zero, `MIN / -1`, OOB index, conversions, `checked_*`/`wrapping_*`/`saturating_*`) | same in every build | same in every build |
| `expr.arith.int.checked` | defers to `types.arith.checked` | defers to `types.arith.release` |
| `flow.for.range.from-overflow` | panics, as `a + 1` does | wraps, as `a + 1` does |
| `flow.panic.sources` | summary: overflow and invalid shifts panic in debug/test only | same summary |
| `module.dbg.release` / `release.debug-build` | `dbg` accepted, no warning | `dbg` is an error (`dbg-in-release`) |
| `module.test.code`, `cli.check.default`, `cli.check.tests` | test code compiles only in test builds (`hd test`); `hd check` skips it unless `--tests` | same: release builds compile executables/library, no test code |
| `cli.profile.default` / `release` / `release.file` | `hd FILE/run/build/test`, REPL: debug (checked) | `--release` on `hd FILE/run/build`: wrapping profile |
| `cli.profile.test` / `test.release` | `hd test`: always checked; `--release` keeps checks, selects pipeline only | n/a (test stays checked) |
| `cli.profile.release.commands` | — | only `hd FILE/run/build/test` take `--release` |
| `cli.profile.pipeline.*` | dev pipeline (fast builds) | optimized pipeline; unobservable except speed, memory, stack/heap depth, backtraces |
| `cli.build.output` | `build/debug/` | `build/release/` |
| `types.generic.instantiation-depth` / `cli.build.depth-error` | `instantiation-too-deep`: limit is implementation-defined, same in every build | same in every build |

## 20. Command Flows Completed

Programs run on one of two engines (`engine.rs`): Node with the JS host
(`host/*.mjs`), or wasmtime in process. `--engine NAME` picks one, else
`HD_ENGINE`, else Node. Making wasmtime the default is #19.

### 20.1 `hd test`

Continuing §7.3 at step 4:

1. Build the executables in the test profile, then the test programs
   (§19.1).
2. Refuse a program whose import list needs a totally denied trait, with
   status 101 ([`cli.cap.total.test`](../../spec/cli/command-line.md#r-cli.cap.total.test)).
3. List cases, apply `--filter` (§19.2). `hd test FILE` with no case, or
   a filter that matches nothing in FILE, is an error.
4. Run the cases (§19.3) on `--jobs` workers, stream results in content
   order through the release cursor, print the summary, and update the
   `--affected` records.
5. Exit 0 when every case passed, 1 when one failed, 101 when `hd` itself
   failed.

### 20.2 `hd run` And `hd FILE`

1. Check (§7.5), build the program in the dev pipeline, or with
   `--release` in the optimized pipeline and the release profile, and
   load its `cwasm` entry.
2. Read the import list; refuse a totally denied need with status 101.
3. Instantiate with the default profile's providers and the grant.
4. Call `hd.init`, then drive `hd.poll` and the reactor (§14.4).
5. Exit with the program's status: the entry's `ExitCode`, 1 for an
   `.Err`, or the profile's panic status.

Implementation differs: there is no `cwasm` entry, because deserializing
wasmtime's precompiled modules is `unsafe`. wasmtime compiles the module
in process on every run, but Cranelift takes each function's machine
code from the module's `clpack` entry when an earlier run compiled it
(docs/compiler/engines-and-test-runner.md §18.2), so a warm run
translates the module and compiles no function.

### 20.3 `hd build`

- Writes the link entry's bytes to `build/debug/NAME.wasm` or
  `build/release/NAME.wasm`, per executable, and
  `build/{debug,release}/files/STEM.wasm` for `hd build FILE`. The file
  is a copy of a cache entry, so an unchanged program writes identical
  bytes.
- A library-only package writes no `.wasm` (§7.5).
- No grant is written into the module
  ([`cli.cap.build`](../../spec/cli/command-line.md#r-cli.cap.build)).

### 20.4 `hd app.wasm`

1. Validate the bytes and check that every import is in the ABI table.
   Otherwise the module was not built by `hd`: status 101 with the
   unknown import named.
2. The grant comes from `--cap` flags alone; refuse totally denied needs.
3. Run as in §20.2.

Planned, not built: also checking the `hd.runtime` section and the entry
exports (crates/hd_run_wasmtime/DESIGN.md §16.4).

### 20.5 The REPL

- A native session retains linked instances in one wasmtime `Store`;
  a Node session retains the equivalent JS instances. Each checked input
  imports earlier initialized code and storage instead of rebuilding its
  values. Checkpoints retain the latest memory/global image and the
  archived results needed for historical reload.
- Each input runs once: its init function evaluates its statements and
  prints an expression input's value with its type
  ([`cli.repl.value`](../../spec/cli/command-line.md#r-cli.repl.value)).
  Host calls are never repeated
  ([`cli.repl.host.once`](../../spec/cli/command-line.md#r-cli.repl.host.once));
  a rebuild serves them from the journal.
- An input that fails to build, panics or is stopped adds nothing to the
  session (`cli.repl.panic.binding`, `cli.repl.panic.declarations`).

### 20.6 The Playground

Continuing §7.9:

1. "Run" in the compiler worker: check, then `Collect`, `Emit` and `Link`
   in `hd_web` with the serial scheduler. The `MemoryStore` holds code
   entries, so a second run after a small edit re-emits few instances.
2. Browser providers (`host/web.mjs` over `host/core.mjs`): `Console` to
   the page, `Clock` and `Random` from the platform, same-origin `Http`
   as a synchronous `XMLHttpRequest`, `TestRunner.temp_dir`, and test
   snapshot files in memory for one run. Every other platform trait is
   unbound, and a module that imports one is refused before it starts.
3. A panic shows the category and message.

Planned, not built: a backtrace mapped through `hd.lines`
(crates/hd_web/DESIGN.md §5).
