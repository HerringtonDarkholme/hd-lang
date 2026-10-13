# REPL On The Rust Compiler: Step Plan

Part of the [compiler design](README.md). Issue #17, step 1.

Status: plan, 2026-10-12. Nothing here is decided. The design is
[live-execution.md](live-execution.md) §2 to §5 (resume, §6, is #18). The
spec rules are [REPL](../../spec/cli/command-line.md#repl) and its three
subsections. The TS REPL is deleted; its behaviour is a checklist only
(§3.3), never code to port.

## Summary

| Question | Answer |
| --- | --- |
| What exists | The driver builds one closed program per call (`Goal::Program`, scripts included). `hd_wasm` links one closed module. `hd_run` has journal rows and framing, no recorder or replayer. `EntryKind::ReplInput` is declared and unused. The playground compiles and runs in one worker |
| The core gap | **Linking across modules.** `hd_wasm::link` imports only host functions, defines its own memory and runtime globals, and exports only entry functions. Every other REPL piece depends on per-input modules that import from earlier ones |
| Compiler cost per input | Today's dev `hd`, warm cache, rebuilding the whole session as one script after adding one input: 20 ms at 50 inputs, 40 ms at 1,000. Compile time is not the problem. Rerunning earlier inputs is: it repeats their CPU and breaks `cli.repl.host.once` unless replayed |
| Runtime cost per input | One small module (about 1 KB with a prelude, 8 to 14 KB without), one Cranelift compile, one instance kept for the session. Memory per instance is unmeasured; job R1 measures it first |
| Order | Measure, then cross-module linking, then the driver's input goal, then the wasmtime session, then `hd` on a terminal. Journal and rebuild after. The playground last, since its Stop needs rebuild |
| Owner questions | 8 (§5). Two gate jobs: Q2 (bang calls at an input's top level) and Q5 (terminal crates) |

## 1. Where Each Piece Lives

### 1.1 What Exists

| Design piece | Today | Citation |
| --- | --- | --- |
| Driver goals | `Program { entry }`, `Tests`, `Analyze`, `CheckTests`; no REPL goal | `crates/hd_driver/src/lib.rs:220-240` |
| One run's state | `Run` is built per `build_packages` call and dropped after it. Only `CacheStore` entries carry over between calls | `lib.rs:630-697`, `lib.rs:771-798` |
| Script roots | A program without a public `main` polls its init as the root: the nearest thing to an input | `Roots::Script`, `lib.rs:613`; detection `lib.rs:4188` |
| Collect | Walks every reachable instance from the roots; code entries hit by instance key, TIR hash and callee reps; the whole module is one `Link` entry | `collect` `lib.rs:4102-4395`; `hd_mono::collect` `crates/hd_mono/src/lib.rs:2115` |
| Program tables | Decodes the TIR of every root-package module's check entry on each run | `program` `lib.rs:3976-4005` |
| Check key | Hashes the deep interface hash of every folder in the module's closure, which includes the module's own folder | `check_key` `lib.rs:2375-2410`; closure `crates/hd_project/src/lib.rs:1072` |
| Scope shadowing | `ModuleScope::overlay` nests one scope in another (used for `tests:` blocks) | `crates/hd_resolve/src/lib.rs:131` |
| Interfaces | Items are `Fn`, `Data`, `Enum`, `Trait`, `Impl`, `Method`, `AssocType`, `Alias`; **no top-level binding entry** | `crates/hd_resolve/src/iface.rs:368-397` |
| Link | One module: host imports (`Sym::Import`) only; defines and exports memory `hd.x`; defines every global (`GSym`: bindings, runtime, canonical data, witnesses); exports only `hd.init`, `hd.poll`, `hd.wake` | `crates/hd_wasm/src/lib.rs:1167-1500`, memory `:1420-1428`, `GSym` `:488-504` |
| Entry exports | `entry`, `script_entry`, `test_entry` | `crates/hd_wasm/src/emit.rs:5720-5800` |
| Runtime globals | `wake`, `fact`, `forbid`, `root` | `crates/hd_wasm/src/rt.rs:140-170` |
| Runtime type identity | A `TypeId` is a key string built from the type's path | `rt.rs:1616` |
| REPL entry kind | `EntryKind::ReplInput`, declared, never emitted | `crates/hd_wasm/src/meta.rs:12` |
| Value printing | `std.format.dbg_text[T]` intrinsic, which `dbg` uses | `lib/std/format.hd:632` |
| Journal | Row kinds, framing, CRC, torn-tail reader, request digest. No recorder, replayer, or cap | `crates/hd_run/src/journal.rs` |
| Embedding API | `Provider`, `Instance`, `drive`. `Provider` is unused: every caller passes `providers: Vec::new()` | `crates/hd_run/src/lib.rs:82-123`; `crates/hd_cli/src/main.rs:382`, `crates/hd_cli/src/engine/wasmtime.rs:124` |
| wasmtime host | Every host import goes through one `call` in `define_with`, which refuses non-function imports and unknown modules | `crates/hd_run_wasmtime/src/host/link.rs:47-100` |
| wasmtime instance | `WasmtimeInstance` owns one `Store` and one instance; `instantiate_host` makes a new `Store` per instance | `instance.rs:11-30`, `engine.rs:113-130` |
| Interrupts | Epoch ticker; the store's epoch callback checks a wall deadline only when one is set | `deadline.rs`, `engine.rs:172-197` |
| CLI | `hd` with no arguments on a terminal prints usage; piped stdin runs a program (`cli.stdin.program`) | `crates/hd_cli/src/main.rs:80-88` |
| Engines | Node is the default; wasmtime is the `wasmtime` cargo feature, off by default | `crates/hd_cli/src/engine.rs:263-269`, `crates/hd_cli/Cargo.toml` |
| JS host | `createCoreHost` keeps one `st` (handles, pending ops, output) across `run(instance, init, poll)` calls; `run` rebinds `st.memory` per instance | `host/core.mjs:417-594`, `:526-527` |
| Browser compiler | `hd_web::Session` keeps sources and a `MemoryStore` across compiles; four exports; no REPL request. New store entries are dropped, not persisted | `crates/hd_web/src/lib.rs:75-185`, `:172`; `exports.rs` |
| Playground | One worker compiles and runs (`worker.ts:46-58`, `rust-runner.ts:317-332`). Stop terminates it (`compiler-client.ts:144`), so the compiler's warm state goes too. A file without `main` already runs as a script (§8.5 of the design is done) | `website/playground/src/` |

### 1.2 What Is Missing

| Design piece | Missing work | Crate |
| --- | --- | --- |
| Session linker (§2.3) | Extern symbols in link: a `Sym::Inst`, `Sym::Part` or `GSym` owned by an earlier module becomes an import; the module exports what it defines under its instance key; memory and runtime globals are imported | `hd_wasm` |
| Collect against `symbols` (§2.3 step 2) | Stop at instance keys the session already has; leave out inits that earlier inputs ran (`module.init.cycle.once`) | `hd_mono`, `hd_driver` |
| Input modules (§2.2) | `Goal::ReplInput`: a synthetic module per input, outside every package folder (§5 Q8a); its scope is `lib.hd`'s private scope overlaid by each earlier input's, newest last | `hd_driver`, `hd_resolve` |
| Bindings across inputs (`cli.repl.declarations.bindings`) | An input interface entry for a top-level binding: name, type, storage path. No folder interface carries one | `hd_resolve`, `hd_check` |
| Expression inputs (`cli.repl.value`) | Lower an expression input to a call of `dbg_text` and a type text fixed at compile time (`Point (input 3)` for a shadowed type) | `hd_driver`, `hd_check` |
| Stale-caller note (`cli.repl.redefine.shadow.report`) | Call edges from earlier inputs' check entries to the shadowed item | `hd_driver` |
| Session store | An overlay `CacheStore`: reads hit `HD_CACHE`, input entries stay in memory, so a session leaves no one-off entries on disk | `hd_cache` |
| Live session (§2.1 `live`) | One `Store` and a `Linker` that gains each input's instance; per-input `hd.init`/`hd.poll`/`hd.wake`; reset of runtime globals after a failed input | `hd_run_wasmtime` |
| Panics (§7.1) | After a trap, `forbid` may still be set (a panic inside `defer` or an init), so the next input's `println` would panic. The session resets runtime globals and `Host::reported` | `hd_wasm`, `hd_run_wasmtime` |
| Stop (§7.3) | An interrupt flag the epoch callback reads, set from Ctrl-C | `hd_run_wasmtime`, `hd_cli` |
| Cancel (§7.2) | A `hd.cancel` export that cancels the root through its `Part::Cancel` | `hd_wasm` |
| Journal (§4) | Recorder at `host/call.rs` and in `core.mjs`; `Wake`, `Block`, `Cancel` rows from the drive loop; a cap | `hd_run`, `hd_run_wasmtime`, `host/` |
| Rebuild (§5.4) | Replayer, the longest-prefix rule, skipped stopped inputs, `:reload` | `hd_run`, `hd_run_wasmtime`, `host/` |
| Prelude (§2.3) | An input 0 per toolchain with the runtime globals, memory and common `dbg_text` instances | `hd_driver` |
| CLI front end | Line reading with continuation, commands, transcript rendering | `hd_cli` |
| Playground | REPL exports in `hd_web`, a separate program worker, JS-side linking, recorder and replayer, the panel, IndexedDB, share links | `hd_web`, `host/`, `website/` |

## 2. Cost Model

### 2.1 Measured Today

Dev profile (`target/debug/hd`, opt-level 1), this machine, one warm
`HD_CACHE` under `target/`. `hd build` of a package whose `src/main.hd` is
the whole session written as one script. "Plus one" adds one statement,
which is what rebuilding the session as one program costs per input.

| Session as one script | First build | Plus one | Peak RSS | Module |
| --- | --- | --- | --- | --- |
| hello (cold cache) | 60 ms | | 33 MB | |
| hello (warm, no change) | 10 ms | | 16 MB | |
| 50 inputs | 20 ms | 20 ms | 22 MB | 23 KB |
| 1,000 inputs (2,000 statements) | 40 ms | 40 ms | 55 MB | 460 KB |

| One input as its own program | Module |
| --- | --- |
| `x := 1 + 2` | 0.7 KB |
| `println("hi")` | 3.3 KB |
| `dbg(1 + 2)` | 8.3 KB |
| `dbg([1, 2, 3])` | 14.3 KB |

Readings:

- The checker and emitter are fast enough that recompiling is never the
  bottleneck in the first release.
- The whole-session module grows by about 0.45 KB per input. wasmtime
  would recompile all of it each input, and the instance would rerun every
  earlier input. Rerunning repeats host calls, which `cli.repl.host.once`
  forbids, so it needs replay on every input. That is the TS design's
  O(n²) cost (live-execution.md finding 1).
- A value-showing input without a prelude carries 8 to 14 KB of `dbg`
  code. The prelude is worth it for module size and first-input latency.

### 2.2 One Input, Per-Input Modules

The plan reuses the one-shot driver: each input is one `build_packages`
call with `Goal::ReplInput`, over the package plus the synthetic input
files, against the session's overlay store. A persistent `Run` across
inputs would need resizable per-module tables; it is not worth it before
the measurements say so.

| Stage | Work for input n | Order |
| --- | --- | --- |
| skim, parse | input n; every other file hits by hash | O(files + n) lookups |
| folder interfaces | package folders hit; input n's interface built; inputs 1..n-1 hit | O(n) lookups |
| check | input n only (1 to 5 ms estimated); the rest hit by check key | O(input) |
| program tables | decodes the TIR of every root-package module, inputs included (`lib.rs:3984-4005`) | O(session TIR): the one superlinear term |
| collect | from input n's roots, stopping at keys in `symbols` | O(new instances) |
| emit | misses only | O(new code) |
| link | input n's module only | O(input module) |
| engine | Cranelift `OptLevel::None` on 1 to 14 KB (estimated 1 to 5 ms); V8 Liftoff well under 1 ms | O(input module) |
| run | the input's own work; earlier inputs never rerun | O(input) |
| journal | one row per host call, about 12 bytes for a clock read | O(host calls) |

The O(session TIR) term is small (tiny inputs decode in microseconds), but
it is quadratic over a session. If job C9's 1,000-input run shows it, the
fix is to keep decoded input bodies in the session between calls, not to
change the model.

### 2.3 Memory Over A Long Session

| Part | Growth per input | 1,000 inputs | Bounded by |
| --- | --- | --- | --- |
| compiler arenas | none: one `Run` per input, dropped after it | 0 | |
| interners | new names and types only | small | |
| input sources, check entries, modules kept for rebuild | about 2 KB check entry plus the module (1 to 14 KB) | 3 to 16 MB | overlay store cap; modules can be dropped and recompiled on rebuild |
| wasmtime instance and code mapping | estimated 20 to 40 KiB (live-execution.md §9.2), unmeasured | 20 to 40 MB | nothing before the `Store` drops |
| GC heap | values the session keeps | user data | shadowed bindings no function reads are cleared (§3.3) |
| journal | host bytes plus 10 to 20 bytes per event | under 1 MB typical | 64 MiB cap |

The bench target for `long-session` is growth of at most 10 MB from input
100 to input 1,000 (`bench/metrics/README.md:121`), about 11 KiB per
input. The estimate is two to four times that. Job R1 measures it before
anything else is built (§5 Q3).

Job R1 measured it (wasmtime 49.0.2 as `hd_run_wasmtime` configures it, GC
on, copying collector, epoch interruption; Apple Silicon, 16 KiB pages,
machine under load; 1,000 inputs of about 25 lines each: a struct type, an
`array.new_data`, two functions, imports of the prelude's memory, global
and function; one `Store`):

| Case | RSS per input | Compile | Instantiate |
| --- | --- | --- | --- |
| compile each input, dev (`OptLevel::None`) | 72 KiB (90 KiB over inputs 1-100, 70 KiB over 100-1,000) | 0.43 ms | 8 us |
| compile each input, release (`OptLevel::Speed`) | 72 KiB | 0.50 ms | 7 us |
| compile once, instantiate 1,000 times | 0.6 KiB | none | under 1 us |
| compile each input, drop the module at once (no instance) | 0.5 KiB after warm-up | 0.43 ms | none |
| compile each input, `memory_init_cow(false)` | 56 KiB | 0.43 ms | 6 us |
| the same plus no address map, unwind info or backtrace | 55 KiB | 0.41 ms | 6 us |

The bound does not hold: a kept instance costs 0.6 KiB, but the compiled
module its instance keeps alive costs about 70 KiB, six times the 11 KiB
allowed and above the 20 to 40 KiB estimate. The cost is the code mapping
(an image of text, read-only data and metadata, rounded to 16 KiB pages),
not the instance, the GC heap or the shared memory; compile time and
optimization level do not change it. Engine settings cut at most a fifth
(`memory_init_cow(false)`, about 56 KiB), and a pooling allocator only
sizes instances, which are not the cost. Compile time (about 0.5 ms) and
instantiate time (under 10 us) are no concern. So Q3 should take the
fallback, but 64 KiB per input is not enough on this machine (72 KiB
measured). Linux uses 4 KiB pages and may fit; that is unmeasured. A
`Store` never frees an instance, so a dead input's module cannot be dropped
inside a session. What lowers the cost is fewer modules: batch several
inputs into one module (the "compile once" row shows instances alone are
cheap), or rebuild the `Store` from the journal when the bound is passed.

Shared linear memory matters: each module that defines its own `hd.x`
gets a 64 KiB page and, on wasmtime, a large virtual reservation. Input
modules import the prelude's memory.

### 2.4 How The Instance Survives Across Inputs

| Engine | Holder | Across a panic | Across Ctrl-C while busy | Across Ctrl-C while waiting |
| --- | --- | --- | --- | --- |
| wasmtime (CLI) | one `Store<Host>` and a `Linker` that gains each input's instance; earlier instances stay referenced | the store stays usable after a trap; the input's exports are not added to `symbols` | epoch callback sees the interrupt flag and traps; the store survives | `hd.cancel` runs `defer` suites and aborts host operations; a `Cancel` row is written |
| Node | `createCoreHost` already shares `st` across `run` calls, but the CLI spawns one `node` per program (`node.rs:119-130`), so a session would need a resident child with a module-passing protocol | the instance throws; the worker lives | not reachable: the child gets no message while Wasm runs | as wasmtime, through the JS glue |
| browser | a program worker holds the instances; the compiler worker keeps `WebAssembly.Module`s | the worker lives | the page terminates the program worker after 250 ms and rebuilds from the journal | as Node |

Recommendation: no Node REPL on the CLI (§5 Q4). The JS host work is still
needed for the browser, and Node runs its tests.

## 3. CLI First, Then The Playground

### 3.1 The CLI REPL

The spec opens the REPL with **`hd` and no arguments, on a terminal**
(`cli.repl.open.terminal`). There is no `hd repl` in the command table.
`guide/COMMANDS.md:147`, `bench/metrics/README.md:121`,
`bench/metrics/scripts/long-session.ts`, commands.md §7.8 and
live-execution.md §4.5 say `hd repl` (§5 Q1).

Rules the CLI must meet, all in `spec/cli/command-line.md`:

| Area | Rules |
| --- | --- |
| scope | `cli.repl.package.lib`, `.dependencies`, `.no-lib`, `.no-lib.main`, `cli.repl.uses.other`, `cli.repl.outside`, `cli.workspace.repl` |
| inputs | `cli.repl.input-types`, `cli.repl.declarations.bindings` |
| host | `cli.repl.host.default-profile`, `.cwd`, `.args`, `.once`; grant from flags only (`cli.cap.source.flags-only`) |
| output | `cli.repl.value`, `cli.dbg.repl` |
| panics | `cli.repl.panic`, `.binding`, `.mutations`, `.declarations` |
| redefinition | `cli.repl.redefine.shadow`, `.shadow.report`, `.impl` |
| interrupts | `cli.repl.cancel`, `cli.repl.stop` |
| rebuild (`:reload`) | `cli.repl.rebuild`, `.prefix`, `.stopped` |

The first CLI release needs no journal: the wasmtime store survives
panics and stops. The journal arrives with `:reload` (jobs J1 and J2).

### 3.2 What The Playground Needs Beyond The CLI

1. **Rebuild is mandatory**, not an extra: Stop of a busy input
   terminates the worker and loses every instance. So the recorder,
   replayer and longest-prefix rebuild must exist in the JS glue, with the
   same row format as `hd_run::journal`.
2. **Two workers.** The playground has one. The REPL needs the
   compiler worker (input interfaces, `symbols`, modules) apart from the
   program worker (instances, heap), so Stop keeps the compiler warm.
3. **`hd_web` REPL exports**: start, input, reset; each answers the
   module bytes, the value's type text, diagnostics and the stale-caller
   note. The wire format grows a request kind.
4. **JS-side linking**: instantiate each input with the host imports plus
   the earlier instances' exports; reset `st.reported` per input.
5. **Persistence**: IndexedDB per session (inputs, journal, toolchain id),
   page reload restores by rebuild, newest 10 sessions kept.
6. **Share links** with the journal when it compresses to 16 KiB or less.
7. **The panel and Try in REPL buttons**, deleted in 59680542 and
   rebuilt in job W5 (the playground README's "The REPL").

### 3.3 Checklist From The TS REPL

Read with `git show 56e4b75e^:src/repl.ts` (also `repl-input.ts`,
`repl-terminal.ts`, `test/ui/repl.test.ts`) and
`git show 59680542^:website/client/repl.ts`. Behaviours to keep:

- input kinds by leading word: declaration, statement, expression;
  `fn(` starts an expression; `use` before a use root is a declaration
- continuation: a line ending in `:` opens a block that an empty line
  ends; open brackets continue; continuation lines indent one level;
  Backspace in leading spaces removes one level; a paste is one entry
- commands: `:help`, `:type EXPR` (checks, never runs or keeps),
  `:reset`, `:quit` (also `:q`, `:exit`, Ctrl-D)
- a rejected binding hides follow-on `unknown-name` errors for its name
  within the same entry only
- values: `dbg` layout, several lines when long, `mut` kept in the type
  (`mut Meters`), `Option.None`, nested options, time types through Debug
- `dbg` lines before the value line; unbounded recursion reports
  `stack-exhausted`; colored values and errors on a terminal
- diagnostics name `repl#n:line:col`

Dropped: rerunning the session per input, the host answer list, going
live after a divergence, and the playground's silent REPL mode for files
without `main`.

## 4. Gaps Against The Design And The Spec

Job C11 measured a whole session (`cargo test -p hd_cli --features
wasmtime long_session_plain -- --ignored --nocapture`, dev profile; Apple
Silicon, 16 KiB pages, machine under load; 1,000 inputs through `Goal::ReplInput`
and `WasmtimeSession::run_input`, each showing `1000000 + i`; the mixed run
adds a function every tenth input and a read of it after). Build and run are
the mean of the 50 inputs before the row; RSS is the process's, by `ps`.
Issue #17 ran the plain session again after making a build read the earlier
inputs from the session (fresh `HD_CACHE` each run):

| Input | RSS (C11) | Build per input (C11) | RSS (#17) | Build per input (#17) | Run per input (C11) | Journal |
| --- | --- | --- | --- | --- | --- | --- |
| 50 | 53 MiB | 16.6 ms | 53 MiB | 13.0 ms | 1.5 ms | 3.3 KB |
| 100 | 59 MiB | 21.7 ms | 58 MiB | 13.9 ms | 1.5 ms | 6.7 KB |
| 200 | 68 MiB | 38.8 ms | 67 MiB | 14.8 ms | 1.6 ms | 13 KB |
| 400 | 94 MiB | 122 ms | 83 MiB | 17.3 ms | 1.6 ms | 27 KB |
| 600 | 92 MiB | 316 ms | 101 MiB | 19.4 ms | 1.7 ms | 41 KB |
| 800 | 127 MiB | 571 ms | 118 MiB | 22.0 ms | 1.7 ms | 54 KB |
| 1,000 | 174 MiB | 925 ms | 136 MiB | 25.5 ms | 1.7 ms | 68 KB |

The mixed run is within 3% on every column (169 MiB, 950 ms at 1,000, C11).
#17's run takes 21 seconds instead of 5.5 minutes.

- Memory: C11 grew 113 MiB from input 100 to 1,000, about 129 KiB per kept
  input, against Q3's 72 KiB (and the 64.8 MiB bound `long-session` now
  uses); #17 grows 78 MiB, about 89 KiB per input. R1's code mapping is
  about 72 KiB of it; the rest is the compiler side (kept modules, check
  entries, interner and session data) and allocator fragmentation, not
  split further. #17 keeps each input's entries in the session (about
  4 KiB per input), yet the total fell, most likely because a build no
  longer allocates per pair of inputs. RSS is not monotone.
- Run time is flat (about 1.6 ms, instantiate included; 1.5 ms on every
  row of #17's run). The journal is
  68 bytes per input (hashes only): 66 KiB for 1,000 inputs, no concern.
- C11's build time per input was superlinear: 17 ms at 50, 925 ms at
  1,000. A sample at input 700 put about half of it in `Run::lookup` (a
  `DiskStore::get` per earlier module), a quarter in `group_init_order`,
  a tenth in `folder_iface`. Each earlier input used every input before
  it, so keys, closures, task edges and the init-order walk were per pair
  of inputs, and every earlier input cost store reads and five tasks.
- #17 (25.5 ms at 1,000, 2.0 times input 50): an input that joins the
  session leaves its `iface`, header and `check` entries and its interface's
  mentions in `hd_driver::Session`; a later build reads each earlier input
  from there in `FolderGraph`, with no key, store read or task. An input
  uses `src/lib.hd` and the input just before it, which gives the same
  closures and orders at one edge per input. `group_init_order` is Kahn's
  order with a heap; an earlier input's init (which ran) is left out of it
  and its TIR is decoded only if collection asks for it; coherence keys
  read the interfaces that carry the impl; the session scope nests each
  input in place. What still grows, about 10 µs per input: decoding each
  earlier input's interface and check entry, its skim, and the program
  tables over every module. The serial scheduler (`HD_JOBS=1`) still picks
  each task by a scan of the ready tasks.
- Linux (4 KiB pages) is unmeasured; `long-session` scales its bound by
  `getconf PAGESIZE`.

Each gap is an owner question in §5. In short:

1. `hd repl` versus `hd` (Q1), and no way to script a session through
   standard input (Q1).
2. The spec's REPL example `read_text!("notes.txt")` is rejected today:
   `bang-call-outside-suspension` (`module.init.script-not-driver`) and
   `expected Path, found string` (Q2).
3. The `long-session` bound against the per-instance estimate (Q3).
4. The CLI's default engine is Node; the REPL needs a resident instance
   (Q4).
5. No line editor or signal handling in the workspace (Q5).
6. No conformance fixture format for sessions (Q6).
7. A module whose initialization panicked inside an input (Q7).
8. Design text that the code contradicts (Q8).

## 5. Owner Questions

**Decided 2026-10-12** (the questions below are kept for the record):

- Q1: bare `hd` on a terminal opens the REPL; piped text runs a program; no `hd repl`. The guide, bench and design text that say `hd repl` are outdated (W1).
- Q2: an input's top level may make bang calls (a driver context, like `main!`'s body).
- Q3: accept about 72 KiB per kept input now (the bench bound follows the page size); later, rebuild the session from the journal past a limit. **2026-10-13:** after C12 a kept input measures about 89 KiB; the owner accepts under 100 KiB per input, and `long-session`'s bound is 100 KiB on 16 KiB pages, scaled by the page size.
- Q4: the CLI REPL runs on wasmtime only.
- Q5: add `rustyline` (Ctrl-C at the prompt) and `ctrlc` (SIGINT while an input runs, for Stop and Cancel).
- Q8a: inputs live outside every package folder. Q8c: the recorder sits at the host call sites. Q8d: design questions 1 and 4 are answered by the spec. Q8e: the playground keeps one worker; Stop rebuilds from the journal (W2 dropped).
- Q8b, replaced: **no redefinition at all.** An input must not redeclare a name an earlier input declared: `duplicate-type` for types, traits and aliases, `duplicate-binding` for functions and bindings (`cli.repl.redeclare.*`, spec edit of 2026-10-12, replacing `cli.repl.redefine.shadow` and `.shadow.report`). Shadowing, the stale-caller note and origin-marked types (C7) are gone.
- Late `Debug` and `Display`, **2026-10-13:** accepted and documented. A session counts as one module, so an input may implement `Debug` or `Display` for an earlier input's type. An implementation added after a type's value was already shown does not change the printer already built for it: code compiled earlier and the `dbg_show` instances the session holds keep printing the type field by field. A type not yet shown uses the new implementation.
- Derivation blocks and templates, **2026-10-13:** a session counts as one module for `annot.block.module` and `annot.template.module` too. An input may write `impl Hash for Point by Structure` for a type that `src/lib.hd` or an earlier input declares, and a later input uses the derived implementation. Other `src/` modules and dependencies keep the rule.

1. **Command name and scripted sessions.** The spec opens the REPL with
   bare `hd` on a terminal, and piped stdin runs a program
   (`cli.stdin.program`). The guide, the bench and two design files say
   `hd repl` and feed it stdin. **Recommendation:** no `hd repl` command.
   Tests and the `long-session` bench drive an in-process session API,
   plus one pseudo-terminal smoke test (`script`). Fix the guide, bench
   and design text to `hd`.
2. **Bang calls at an input's top level.** The spec's REPL example calls
   `read_text!` at top level, but a script's top level is not a driver
   (`module.init.script-not-driver`). The example also passes a `string`
   where `read_text` takes a `Path`. **Recommendation:** an input's top
   level is a driver context, as `main!`'s body is, with its row inferred
   under the default profile. The design already polls each input as an
   entry (§2.3 step 5). Log both points in
   `audit/compiler/diagnostic-notes.md` for the next spec pass. Job C6
   waits for this answer.
3. **`long-session` for the REPL** (design open question 2). The bench
   allows about 11 KiB per input; the estimate is 20 to 40 KiB on
   wasmtime. **Recommendation:** decide after job R1 measures it. If it
   is over, define the REPL row as a fixed bound per input (64 KiB),
   update the bench target, and leave compaction for later.
4. **Engines.** **Recommendation:** the CLI REPL runs on wasmtime only. A
   build without the `wasmtime` feature says the REPL needs it. Whether
   wasmtime becomes the default feature is a separate decision.
5. **Terminal crates.** The workspace has no line editor and no Ctrl-C
   handler, and our code forbids `unsafe`. **Recommendation:** add
   `rustyline` (history, multi-line validation for the continuation
   rules) and `ctrlc` (interrupt while an input runs). Both keep `unsafe`
   inside the dependency. Job C7 waits for this answer.
6. **Session fixtures.** The spec owns fixture formats, and it is frozen.
   **Recommendation:** Rust tests named by rule ID for now. Log a
   proposal for a `cli` session fixture (an inputs file and the expected
   transcript) in diagnostic-notes for the next spec pass.
7. **A module whose initialization panicked.** If input 4 is the first to
   reach a module and that module's init panics, its storage lived in
   input 4's discarded instance. `module.init.cycle.once` says "exactly
   once per program instance". **Recommendation:** the next input that
   reaches the module initializes it again. Non-entry init is
   requirement-free (`module.init.requirement-free`), so no host call
   repeats. The REPL notes the retry.
8. **Design corrections** (approve as a batch; live-execution.md is edited
   in job W1):
   - a. Inputs live outside every package folder, not in `src`. The check
     key hashes the module's own folder interface (`lib.rs:2379-2384`),
     so a declaration in `src` would recheck every module of `src` on
     each input. `lib.hd`'s private scope comes in through the session
     scope instead (`cli.repl.package.lib`).
   - b. No session-wide runtime type id table. A `TypeId` is keyed by
     path (`rt.rs:1616`), and each input module has its own path, so a
     shadowed `Point` and its replacement already differ.
   - c. The recorder sits at the wasmtime host's one `call` site
     (`host/link.rs:47-100`) and in `core.mjs`'s import table, not on
     `hd_run::Provider`, which nothing uses.
   - d. Design questions 1 and 4 are answered by spec pass S1e
     (`cli.repl.redefine.shadow`, `cli.repl.rebuild.stopped`). Only
     question 2 stays open (Q3 above).
   - e. The playground has one worker, not the two of commands.md §7.9;
     splitting it is job W2.

## 6. Step Plan

Each job is one agent task of at most about an hour, in order. "Gate"
names the question whose answer the job waits for. Every compiler job
reports the footprint of a tiny program (WAT, functions, build time)
before and after; programs that are not REPL inputs must link to
identical bytes.

### Runtime Measurement

| Job | Work | Done check |
| --- | --- | --- |
| R1 | Spike, not kept: instantiate 1,000 hand-written GC modules in one wasmtime `Store`, each importing memory, a global and a function from a prelude instance. Report RSS growth per instance, compile time and instantiate time | Numbers added to §2.3 of this file. Q3 can be answered |

### Compiler

| Job | Work | Done check |
| --- | --- | --- |
| C1 | `hd_wasm::link` extern symbols: a link mode in which `Sym::Inst`, `Sym::Part` and `GSym` owned elsewhere become imports, defined ones are exported by key, and memory and runtime globals are imported | Unit test: module B imports a function and a global that module A exports; both validate; a wasmtime test instantiates B against A and calls it. Tiny program bytes unchanged |
| C2 | `hd_mono::collect` takes the session's known keys and run inits; hits become externs, inits already run are left out | Driver test: input 2 calling input 1's function emits no instance for it and imports it |
| C3 | Overlay `CacheStore` in `hd_cache`: reads fall through to the inner store, writes stay in memory | Test: a session's builds add no entries to the disk store |
| C4 | `Goal::ReplInput` with a synthetic module per input outside package folders; scope = `lib.hd`'s private scope overlaid by earlier inputs (`ModuleScope::overlay`); panicked inputs leave the source set | Driver test: 6 inputs of the spec's Redefinition example each build a module; `modules_checked` lists only the newest input |
| C5 | Top-level bindings in input interfaces; declarations read earlier bindings through imported globals | Tests for `cli.repl.declarations.bindings` and the `prices` example of live-execution.md §2.2 |
| C6 | Input kinds: an expression input lowers to `dbg_text` plus a type text; mixed declarations and statements; driver context at top level. Gate: Q2 | Tests: `[3, 6] : List[usize]`; the value forms of §3.3; `cli.dbg.repl` order |
| C7 | Dropped (no redefinition): instead, `cli.repl.redeclare.binding` and `.type`, reporting `duplicate-binding` / `duplicate-type` for a name an earlier input declared | Tests: redeclaring `price`, a binding and a `data` type each fail with the right code; a panicked input's names stay free |
| C8 | Prelude module (input 0) with runtime globals, memory and common `dbg_text` instances, cached per toolchain | The `1 + 2` input module is under 2 KB; first-input latency reported |

### wasmtime Session And CLI

| Job | Work | Done check |
| --- | --- | --- |
| S1 | `hd_run_wasmtime` session: one `Store`, a `Linker` that gains each instance, `define_with` skips session modules, per-input drive, reset of runtime globals and `Host::reported` after a failed input | Test: the spec's `log` panic example; `log` shows the push; a later `println` works after a panic in `defer` |
| S2 | Stop: an interrupt flag read by the epoch callback | Test: a loop input stopped from another thread; the next input runs; earlier values remain |
| S3 | Cancel: the `hd.cancel` export through the root's `Part::Cancel` | Test: a waiting input is cancelled, its `defer` runs, the session continues (`cli.repl.cancel`) |
| C9 | `hd` on a terminal opens the REPL (wasmtime build): reading with continuation, commands, transcript, `repl#n` diagnostics, Ctrl-C wiring. Gates: Q1, Q4, Q5 | In-process session tests per `cli.repl.*` rule; one pseudo-terminal smoke test; `hd help` unchanged |
| C9a | Syntax highlighting at the prompt (owner, low priority): rustyline `Highlighter` colouring `hd_syntax` lexer tokens with the website's token classes | Test: a highlighted line keeps its text when ANSI codes are stripped; keywords, strings and comments get distinct colours |
| C9b | Tab completion, only if cheap (owner, lowest priority): rustyline `Completer` over keywords and the session scope's names (`lib.hd` plus earlier inputs) | Test: `pri<Tab>` offers `price` after it is declared; keywords complete |
| C9c | Optional, lowest priority: `name.<Tab>` for a bare session binding lists its fields and inherent methods (binding types from C5, `hd_check::conflicts::inherent_methods`); no trait methods, no compound expressions | Test: `log.<Tab>` on a `List[string]` binding offers `push`; a `data` binding offers its fields |
| C10 | Package mode and workspace mode scopes | Tests for `cli.repl.package.*`, `cli.repl.outside`, `cli.workspace.repl` |
| C11 | `long-session` run on the session API: 1,000 inputs, RSS every 50 | Done: numbers in §4. Memory is 129 KiB per input, above Q3's 72 KiB; build time was superlinear (lookups, init-order grouping), near flat since #17 (13 ms at 50, 26 ms at 1,000; 89 KiB per input) |

### Journal And Rebuild

| Job | Work | Done check |
| --- | --- | --- |
| J1 | Recorder at the host `call` site; `Wake`, `Block`, `Cancel`, `Input`, `End` rows from the session; in-memory journal with the 64 MiB cap | Test: a session's rows read back; a clock read costs about 12 bytes; past the cap the REPL says once that rebuild is off |
| J2 | Replayer and rebuild (longest prefix, skipped stopped inputs), `:reload` after package edits | Tests for `cli.repl.rebuild`, `.prefix`, `.stopped`; no host call repeats |

### Playground

| Job | Work | Done check |
| --- | --- | --- |
| W1 | Docs: live-execution.md corrections (Q8), the playground README's REPL section, guide and bench text (after Q1) | `pnpm run website:build` passes; `grep -rn "hd repl" guide bench future-work website/playground` finds only this plan |
| W2 | Dropped (owner: one worker; Stop rebuilds from the journal) | n/a |
| W3 | `hd_web` REPL exports and wire format | Done: goal bytes 4 to 6 over the four exports (playground-wasm.md §3.1); `hd_web` tests build the Redefinition example without redeclaration, one module per input with its session keys. No type text or note: the value line goes to stdout, and redeclaring is an error |
| W4 | JS host: per-input linking in `core.mjs`, recorder and replayer with the `hd_run` row format | Done: `createSession` in `host/core.mjs` (`createNodeSession`, `createWebSession`) links each input to earlier inputs' session keys under `hd.s`, records at the import call sites with `host/journal.mjs`, and rebuilds as J2 does. `hd_run_wasmtime`'s `a_session_runs_and_replays_the_same_on_node` runs one session on both engines with the same transcript, and each journal replays on the other into the same bytes; `repl-session.test.ts` and `journal.test.ts` cover the rest |
| W5 | The REPL panel and Try in REPL buttons; Stop as cancel, then terminate and rebuild after 250 ms | Done: the playground's REPL view and the site's docked panel (`repl-panel.ts`) over `repl-host.ts` in the compiler worker; the page numbers inputs and `hd_web`'s repl input takes the number, so a rebuild in a fresh worker keeps them. `website:e2e` (`e2e-repl.ts`): value, binding, panic, Stop of a loop restored from the journal, Stop of a sleep cancelled with its `defer`, `:reload`, `:quit`, Try in REPL; `repl-host.test.ts` covers the worker protocol |
| W6 | IndexedDB sessions and share links | e2e: a reload restores the session; a link with a journal shows the sender's output |

Order and slack: R1, C1 to C5 and S1 need no answer and can start now.
C6, C9 and W1 wait on Q1, Q2, Q4 and Q5. The CLI REPL is usable after
C9; the playground panel after W5.
