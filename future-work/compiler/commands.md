# New Compiler Design: Command Flows

Part of the [compiler design](README.md).

## 7. Command Flows

### 7.1 `hd check` (Package Mode)

1. Find the package or workspace ([Package Mode](../../spec/cli/command-line.md#package-mode)).
   Parse `hd.toml`, select dependency versions, verify `hd.sum`, and
   fetch what is missing ([Fetching](../../spec/cli/command-line.md#fetching)).
   A manifest error is reported here and stops the run, before any cache
   lookup. Then compute `manifest_key` (§5.3).
2. Open the session: `toolchain_key`, the embedded std pack (mapped, not
   decoded), the cache store. The thread pool starts only on a fast-key
   miss, and no Wasm engine is created by `hd check` at all. `hd
   --version` and `hd help` return before any of this (Codex review, P4).
3. Walk and stat the files; diff against the manifest (§5.5).
4. Compute `fast_key`. On a hit, print the stored `pkgres` and stop.
5. Build the folder graph from manifest use lists, plus skims of the
   changed files.
6. Bottom-up over folders: compute `iface_key`. When it equals the
   last-run record's key (§5.5.1), take the folder's hashes from the
   record, with no open. Otherwise look it up; on a hit, read the entry's
   header for its hashes; on a miss, skim the folder's files and run
   `FolderIface`.
7. For each module, compute `check_key`. When it equals the record's
   key, the module's result is the previous one and nothing is opened.
   Otherwise look it up; on a hit, take its result; on a miss, try early
   cutoff (§5.3.1, once enabled), and else parse it and run M1 to M3.
8. Run `Coherence` once, over the traits whose `coh_key` changed, and
   `InitOrder` for folders whose `init_key` changed. Unchanged parts are
   copied from the previous `graph` entry.
9. Print diagnostics in content order once every task that can add one
   is done (§6.5), then the summary. Then the I/O thread finishes
   publishing: the `check` entries, the `graph` and `pkgres` entries, and
   the manifest with its last-run record (§5.4, "Print, then publish").
   Eviction, if a shard is over budget, runs last.

Touched: `iface`, `check`, `graph`, `pkgres`, and the manifest. Reads
are proportional to what changed (§5.9). Dependency bodies are never
parsed: with answer 13, interfaces come from syntax alone, so the
research's "dependency bodies skipped" now holds cold as well as warm.

### 7.2 `hd check FILE`

1. As steps 1 to 6 above, but only for the folders that FILE's module
   deeply depends on, plus its own folder.
2. Check only FILE's module (its `check` entry, and its test overlay when
   FILE is test code).
3. Coherence runs only for traits with an impl in FILE.
4. Output: FILE's diagnostics, plus one note per used folder whose
   interface has errors ("src/shop has 2 errors; run hd check").
5. A FILE under no root is a single-file program
   ([`cli.package.no-root`](../../spec/cli/command-line.md#r-cli.package.no-root)):
   one module, std only, no manifest.

### 7.3 `hd test` (Up To D2)

1. `hd check --tests`: also test overlays, test modules (one test unit),
   integration test programs (each its own root over the package's public
   interfaces, [`module.test.integration.view`](../../spec/lang/10-modules.md#r-module.test.integration.view)),
   and doc tests.
2. On errors, print them and stop with status 101.
3. Build the **test plan**: the list of test programs and their cases.
   One unit-test program per package, holding every module's `tests:`
   cases and doc tests, with an init export per module (§19.1); one
   program per integration test file. Registration names are string
   literals ([Registration Functions](../../spec/lang/10-modules.md#registration-functions)),
   so the checker lists each program's cases without running anything.
   Only the row count of an `it_each` case is learned at run time.
   `--filter` is applied to this list.
4. Hand the plan to D2, which builds, runs and reports. The package's
   executables are built first ([`cli.test.builds-executables`](../../spec/cli/command-line.md#r-cli.test.builds-executables)).
   §20.1 continues this flow.

### 7.4 `hd test --affected`

`build/.hd/last-test` records, per **case group**, the fingerprint of the
last run in which **every** case of that group ran and passed. A case
group is one module's unit cases and doc tests, or one integration test
file. With one unit-test program per package (§19.1), a program-wide
fingerprint would rerun every unit test after any edit, so the
fingerprint is per group:

```text
group_fp(g) = H("affected", toolchain_key, tier, profile, test options,
                the overlay's TIR content hash for g's module (or g's file),
                sorted [(module path, tir_content_hash)] of the modules g's
                cases and init reach in the use graph)
```

That is the `prog_key` (§11.3) that a program of g's cases alone would
have. It covers, through TIR content hashes, every source change that
can change what g's cases run. A comment edit changes no content hash,
so it selects nothing (systems review, finding 4). The test options are
the seed and the time limit.

1. Run `hd check --tests` as `hd test` does. This also computes every
   module's TIR content hash.
2. Select each case group whose fingerprint differs from its record or
   has no record. A group that failed, or ran only partly under a filter,
   has no record, so it is selected until it passes in full.
3. Build the package's unit-test program filtered to the selected
   groups' cases, as `--filter` does (§19.1), plus the selected
   integration programs, and run them. After the run, write a record for
   each group that ran in full and passed, and remove the record of each
   group that failed.

**Cost.** Computing about 100 fingerprints hashes about 100 lists of at
most 100 pairs: under 1 ms. The build is one filtered program, whose
code entries all come from the full program's packs.

This replaces one package-wide "last observed sources" record (Codex
review, A6). That record missed dependency and manifest changes, and a
filtered run could advance it past a module whose tests never ran.
Removed modules need no old graph edges: the fingerprint of every group
that reached them changes.

### 7.5 `hd run`, `hd build`

- **`hd run [NAME]`:** check the package without test code, then hand the
  chosen executable's entry module to D2. D2 builds from the reachable
  items and runs under the default profile.
- **`hd build`:** check the whole package. On errors, stop. Otherwise D2
  writes `build/debug/NAME.wasm` per executable, or `build/release/` with
  `--release`.
- **A library-only package** (owner, 2026-10-07): `hd build` checks it and
  writes only its interface and cache entries. It writes no `.wasm` file.

§20.2 to §20.4 continue these flows.

### 7.6 `hd fmt`, `hd fix`

**`hd fmt`.** Per file, in parallel:

1. Skip the file when its manifest record says its current source hash is
   already formatted (a per-file bit, mine).
2. Full parse. A file with a syntax error is not formatted; its first
   syntax error is printed. Whether that is right is open question 6.
3. Lower the green tree to a Wadler-style document and print it within the
   line width. Comments are trivia in gaps and are kept.
4. Write changed files atomically. `--check` lists files that would
   change.

No type checking and no cache entries. Idempotence is tested on every
fixture (`fmt(fmt(x)) == fmt(x)`).

**`hd fix`.**

1. Run `hd check` with fix-its.
2. Take the `Exact` fix-its. Where two overlap, the first in content order
   wins.
3. Apply them per file, re-parse each edited file, and drop any fix whose
   file gains a syntax error.
4. Re-check incrementally: only edited modules and their dependents miss
   the cache.
5. If an edited module has a diagnostic with a new root key, revert that
   file's fixes one at a time until it has none.
6. Repeat up to 3 rounds, write the files, and print what was applied and
   what remains.

### 7.7 `hd doc`

- **Whole package:** build interfaces (no bodies), then render
  [HD_DOC.md](../HD_DOC.md) pages from `iface` entries plus doc comment
  text, read from source through the skeleton's ranges.
- **`hd doc NAME`:** resolve NAME through the package's exports or a
  dependency's, skim the one declaring file for its doc comment, and print
  the item from its interface record.
- Doc comments are never in an interface hash, so a doc edit rechecks
  nothing.

### 7.8 `hd repl`

1. In package mode the session acts as code inside `src/lib.hd`
   ([`cli.repl.package.lib`](../../spec/cli/command-line.md#r-cli.repl.package.lib)):
   run `ModulePrep` for `lib.hd` to get its private scope.
2. Each input is a synthetic file `repl#n` in folder `src`. Its scope is
   `lib.hd`'s scope, the uses of every earlier input, and the earlier
   inputs' top-level bindings with their fixed types
   ([`cli.repl.input-types`](../../spec/cli/command-line.md#r-cli.repl.input-types)).
3. Check it as top-level statements of a one-input module, then hand it to
   D2, which runs it in the session's persistent program instance.
4. Each input's arenas are freed after it runs. Only interned names and
   types persist, and they grow with new names only, which keeps
   `long-session` flat.

### 7.9 The Playground

```text
 page (main thread)          compiler worker                  program worker
 ──────────────────          ───────────────                  ──────────────
 editor edits ─────────────► SourceSet (JS map, versioned)
 "check" ──────────────────► SteppingScheduler.run_for(...) loop
                             ◄── JSON lines (same as the CLI)
 "run" ────────────────────► check + D2 build ──► Wasm bytes + import list ──► instantiate on V8
                                                                              with generated JS host
 console, http ◄──────────────────────────────────────────────────────────── host calls
 "stop" / timeout ──────────────────────────────────────────────────────────► terminate worker
```

1. The compiler worker loads `hd_web`'s Wasm once; std is embedded.
2. When the worker starts, the page's IndexedDB entries fill the
   `MemoryStore` once (§5.8), not before each run.
3. Sources cross as UTF-8 bytes per path with a version counter; unchanged
   paths are not re-sent.
4. Checking uses the stepping scheduler. A newer edit cancels the run
   between slices; the session keeps its interners and frozen interfaces
   for the next run.
5. "Run" hands the built Wasm bytes to a separate program worker, which
   instantiates them with D2's generated JS host. Stopping or timing out a
   program terminates that worker only, so the compiler's warm state
   survives (mine).
6. After each run the worker drains new cache entries to IndexedDB.

## 20. Command Flows Completed

### 20.1 `hd test`

Continuing D1's §7.3 at step 4:

1. Build the executables in the test profile, then the test programs
   (§19.1).
2. Refuse a program whose import list needs a totally denied trait, with
   status 101 ([`cli.cap.total.test`](../../spec/cli/command-line.md#r-cli.cap.total.test)).
3. List cases, apply `--filter` (§19.2). `hd test FILE` with no case, or
   a filter that matches nothing in FILE, is an error.
4. Run the cases (§19.3), stream failures in content order, print the
   summary, write `build/.hd/last-test` for `--affected`.
5. Exit 0 when every case passed, 1 when one failed, 101 when `hd` itself
   failed.

### 20.2 `hd run` And `hd FILE`

1. Check (D1 §7.5), build the program in the debug or release tier, and
   load its `cwasm` entry.
2. Read the import list; refuse a totally denied need with status 101.
3. Instantiate with the default profile's providers and the grant.
4. Call `hd.init`, then drive `hd.poll` and the reactor (§14.4).
5. Exit with the program's status: the entry's `ExitCode`, 1 for an
   `.Err`, or the profile's panic status.

### 20.3 `hd build`

- Writes the link entry's bytes to `build/debug/NAME.wasm` or
  `build/release/NAME.wasm`, per executable, and
  `build/{debug,release}/files/STEM.wasm` for `hd build FILE`. The file
  is a copy of a cache entry, so an unchanged program writes identical
  bytes.
- A library-only package writes no `.wasm` (D1 §7.5).
- No grant is written into the module
  ([`cli.cap.build`](../../spec/cli/command-line.md#r-cli.cap.build)).

### 20.4 `hd app.wasm`

1. Validate the bytes; check `hd.runtime`, the entry exports, and that
   every import is in the ABI table. Otherwise the module was not built
   by `hd`: status 101 with the missing export or unknown import named.
2. The grant comes from `--cap` flags alone; refuse totally denied needs.
3. Precompile through the `cwasm` cache, keyed by the module's hash, then
   run as in §20.2.

### 20.5 The REPL

- The session is one `Store`. Each input is checked (D1 §7.8) and built
  as its own small Wasm module, which imports the earlier inputs' exported
  bindings and functions and exports its own. Wasm GC types canonicalize
  across modules, so values flow between inputs unchanged.
- Each input runs once: its init function evaluates its statements and
  prints an expression input's value with its type
  ([`cli.repl.value`](../../spec/cli/command-line.md#r-cli.repl.value)).
  Host calls are never repeated
  ([`cli.repl.host.once`](../../spec/cli/command-line.md#r-cli.repl.host.once)).
- A panic in an input is open question 6.
- Each input's compiler arenas are freed after it runs; only the pool and
  the session's instances grow, which `long-session` measures.

### 20.6 The Playground

Continuing D1's §7.9:

1. "Run" in the compiler worker: check, then `Collect`, `Emit` and `Link`
   in `hd_web` with the stepping scheduler. The `MemoryStore` holds code
   entries, so a second run after a small edit re-emits few instances.
2. The bytes and `hd.runtime` go to the program worker (§18.4).
3. Browser providers: `Console` to the page, `Clock` and `Random` from the
   platform, same-origin `Http` with `fetch` (or synchronous XHR in
   synchronous mode, §17.6), an in-memory file system, and the
   playground's grants (HOST_CAPABILITIES S7).
4. A panic shows the category, message and a backtrace mapped through
   `hd.lines`; the source map is built on demand (§15.5).
