# Live Execution: REPL, Journal And Replay

Part of the [compiler design](../compiler/ARCHITECTURE.md).

Status: built (#17). Sections 2, 4, 5, 7, 8 and 9 describe the shipped
design, and the code cites them by number. Durable resume (#18) and hot reload
(#182) are parked; their designs are in those issues. Journal checkpoints
(#646) are decided (owner, 2026-10-25): an initialized base module with
extension modules, using a memory snapshot inside it; built after the
HTTP and CLI work.

## Summary

One mechanism serves the REPL, replay and resume: an **execution journal**
of every nondeterministic input. The REPL runs live, and the journal lets
it rebuild. Redefinition does not go through the journal: an input must
not redeclare an earlier input's name (`cli.repl.redeclare.*`).

| Question | Answer |
| --- | --- |
| What is a session? | Its inputs (source text), a live program instance, and a journal |
| How does an input run? | Checked against the earlier inputs' interfaces, compiled to one small Wasm module, linked into the session's one `Store`, run once |
| Redefinition? | None. Redeclaring a type, trait or alias is `duplicate-type`; a function or binding is `duplicate-binding` |
| What is journaled? | Host-call results, wake batches, cancellations, and an input marker per input; nothing else is nondeterministic ([`req.determinism.depends`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends)) |
| Where is it recorded? | At the host call sites. No compiled hooks, so the same debug or release binary can be recorded and replayed |
| When does the REPL replay? | Only to rebuild: a browser Stop of a busy input, a page reload, a share link, `:reload` after package files change, and the 1000-input bound |
| What if a rebuild diverges? | It keeps the longest prefix of inputs that check and replay identically; the first divergent input and every later one are dropped and reported. Host calls are never repeated live |
| Later | Durable resume (#18), checkpoints (#646), hot reload (#182), notebooks, simulation testing, metered stops |

## 1. Prior Art

| System | Mechanism | Lesson for hd |
| --- | --- | --- |
| GHCi, OCaml, Scala, Kotlin, Swift, evcxr | Typed REPLs that shadow redefinitions | Shadowing is sound but gives "expected `T`, found `T`" and stale callers; hd forbids redefinition instead |
| Clojure, Python, Erlang, Julia | Late binding through vars, globals or reloaded modules | Callers see fixes, but data built by old code goes stale invisibly |
| Pluto.jl, Observable, marimo | Reactive notebooks rerun dependent cells live | Rerunning live repeats effects |
| Temporal, Restate, Azure Durable Functions | Event history or step journal; replay serves recorded results | Replay at a new code version needs versioning or refusal; histories need truncation |
| rr | Records every kernel input, replays exactly | Record at the one boundary that carries nondeterminism |
| Elm debugger | Records messages; exported histories replay | A changed model type invalidates an old history |

hd records at the host boundary, checks each request against the record
(as Temporal compares commands), refuses replay at a changed code version,
and caps journal growth.

## 2. The Session Model

### 2.1 What A Session Is

```text
Session
├─ base         package snapshot: lib.hd's scope, interface hashes, toolchain, profile, grant
├─ inputs[i]    source text, kind, input interface, module hash, outcome
├─ symbols      instance key → (input module, export name)    the session linker
├─ live         one Store (wasmtime) or one program worker (browser): instances, heap
└─ journal      rows keyed by (session id, event index)
```

The inputs and the journal are the truth; the live instance is a cache of
what replaying them would build, and each input costs only its own check,
compile and run. The session acts as code inside `src/lib.hd`
([`cli.repl.package.lib`](../../spec/cli/command-line.md#r-cli.repl.package.lib)),
so it is one program instance
([`module.init.program-instance`](../../spec/lang/10-modules.md#r-module.init.program-instance)).

### 2.2 Checking One Input

Each input is a synthetic file `repl#n`, outside every package folder,
checked as the top-level statements and declarations of a one-input
module. Lookup goes from the newest input's interface to the oldest, then
the uses earlier inputs declared, then `lib.hd`'s private scope, then
std's prelude. No name appears twice, so order matters only for uses.

- **Input interface.** After an input checks, its new names (items and
  top-level bindings with their types) are frozen into a small interface,
  as a folder interface is. Later inputs read it and never re-check it;
  types are fixed per input
  ([`cli.repl.input-types`](../../spec/cli/command-line.md#r-cli.repl.input-types)).
- **Declarations see earlier bindings**
  ([`module.init.storage`](../../spec/lang/10-modules.md#r-module.init.storage)).
  Definite initialization holds because an earlier input's bindings have
  already run.
- **Input kinds.** A declaration input adds items, a statement input runs,
  an expression input runs and shows its value
  ([`cli.repl.value`](../../spec/cli/command-line.md#r-cli.repl.value)); an
  input may mix them, as a script does.
- **Arenas.** Each input's compiler arenas are freed after it runs; only
  interned names and types and the frozen interfaces persist.

```text
hd> let prices: mut Map[string, u32] = Map.new()
hd> fn price(item: string) -> u32:          # reads input 1's binding
...     prices.get(item).unwrap_or(+0)
hd> prices.insert("tea", +3)
hd> price("tea")
3 : u32
```

### 2.3 Compiling And Linking One Input

One Wasm module per input, in one `Store`:

1. **Roots.** The input's top-level statements form its init body.
2. **Collect.** Each reachable instance key (stable path plus type
   arguments) is looked up in `symbols`. A hit becomes a Wasm import from
   the module that defined it; a miss is emitted into this input's module.
3. **Storage.** Module storage exists once per session. The first input
   that reaches a module emits its storage globals and runs its init group
   ([`module.init.cycle.once`](../../spec/lang/10-modules.md#r-module.init.cycle.once));
   later inputs import them.
4. **Exports.** The module exports every instance and global it defines,
   under its instance key, and the session records them in `symbols`.
5. **Run.** Instantiate with pre-resolved imports, then drive the root
   through `hd.poll` and `hd.wake` as an entry (`RuntimeMeta.entry` is
   `ReplInput`).

```text
prelude.wasm      cached per toolchain: runtime globals, panic globals, wake table, common std instances
repl#1.wasm       imports prelude;          defines global prices, exports it
repl#2.wasm       imports prices;           defines price, exports it
repl#3.wasm       imports prices, Map.insert instance
repl#4.wasm       imports price, dbg_text[u32]
```

One `Store` links every input, so a value built in input 1 flows into
input 4 unchanged. With no redefinition a call always goes to one fixed
item, so direct imports are enough. An input module's cache entries are
keyed by its TIR hash plus the hashes of the symbols it imports, so a
rebuild of the same inputs recompiles nothing. The prelude removes most of
the first input's latency.

## 3. Redefinition

There is none. An input must not redeclare a name an earlier input
declared (`cli.repl.redeclare.*`). Replacing a definition and replaying
the session would silently change earlier outputs, diverge whenever the
change alters a host call, can make earlier inputs ill-typed, and costs the
whole session's CPU per edit. Shadowing and late binding were weighed and
not adopted.

## 4. The Journal

### 4.1 Why The Host Boundary Is Enough

An instance's behavior depends only on its compiled program, its profile,
its entry arguments, and the ordered host-call results, wakes and
cancellations
([`req.determinism.depends`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends)).
File, network and process I/O, clock readings, random draws, `hd.wake`
batches, `hd:rt/block` results, outside cancellation, and `Env`/`Args`
calls all cross the host API, and code between host calls has no other
source of nondeterminism
([`req.determinism.no-other-source`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.no-other-source)).
A NaN's payload is canonical before it can reach the host
([`types.display.nan-canonical`](../../spec/lang/04-type-system.md#r-types.display.nan-canonical)).

So the recorder sits at the host call sites and needs no `hd:hook` imports
or special build. The driver loop writes one `Wake` row per `hd.wake`, and
`hd:rt/block` one `Block` row.

### 4.2 What Is Recorded

| Row | Written when | Payload |
| --- | --- | --- |
| `Header` | once | format number, execution id, code identity (4.4), toolchain, profile, grant, limits, entry arguments, working directory |
| `Input` | a REPL input starts | input index, source hash, module hash |
| `Call` | a provider method returns | method id, request digest, `Ready(result bytes)` or `Pending(handle)` |
| `Finish` | a `.finish(h)` returns | handle, result bytes |
| `Wake` | the driver calls `hd.wake` | the poll count, the batch of handles |
| `Block` | `hd:rt/block` returns | the batch of handles |
| `Abort` | the program aborts a host operation | handle (checked on replay, not served) |
| `Cancel` | the host cancels the root | the poll count |
| `End` | an input or the run ends | value shown, exit status, panic category, cancelled, or stopped |

Scalars are stored in their exchange encoding (LEB128, or raw IEEE 754
bits). Structured results are the bytes the host wrote into the exchange
buffer. A request digest is a 64-bit hash of the method id and argument
bytes; arguments of 8 bytes or less are stored as is. Output methods are
recorded with a digest of their text, which replay produces again.

### 4.3 Format

```text
journal = Header Row*
Row     = kind: u8, index: uleb, len: uleb, payload: [u8; len], crc32: u32
```

The journal is append-only; each row carries its index and a checksum, and
a torn tail is cut at the last valid row.

### 4.4 Keys And Identity

| Key | Value |
| --- | --- |
| execution id | 128 random bits per run or session, in the header |
| event index | the row's position, from 0 |
| code identity (`hd run`) | the hash of the program's Wasm bytes ([`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program)) |
| code identity (REPL) | per input, the hash of the module it ran, in its `Input` row |

### 4.5 Where It Lives

The reader, writer, recorder and replayer live in `hd_run` and
`host/journal.mjs`. The terminal REPL keeps the journal in memory for the
session; the playground keeps it in IndexedDB, written after each input.

### 4.6 Size And Growth

The journal grows with the bytes the host delivers plus about 10 to 20
bytes per event; a typical session is under 1 MB. Each session has a cap
(64 MiB on the CLI). Past it, recording stops and the REPL says once that
the session can no longer be rebuilt; the live session continues.

## 5. Replay

### 5.1 Deterministic Re-Execution

```text
on provider call (m, args):
    row = next row; it must be Call
    if row.method != m or row.digest != digest(args): divergence
    if m is an output method and the mode shows output: perform it
    return row.result                       ;; Ready bytes, or the Pending handle
on finish(h):      the next Finish row for h
on driver turn:    deliver the next Wake row's batch at its poll count; never wait
on block:          the next Block row's batch
on abort(h):       check against the next Abort row; do nothing
end of journal:    the record is exhausted; a rebuild switches to live providers
```

Time is compressed: a replayed timer or wait completes at once. A REPL
rebuild is silent. A panic replays at the same point with the same partial
mutations.

### 5.2 Divergence

A divergence is a request that does not match the next row: another
method, other arguments, a poll where the journal has a wake, or a run that
ends before its `End` row.

```text
error[replay-divergence]: input 4 called Clock.now_ms, but the record has FsRead.read_text("prices.csv")
  --> repl#4:2:5
note: the session kept inputs 1 to 3; inputs 4 to 9 were dropped and are in the history
```

A replay never goes live after a divergence; the context decides what
happens next. Every output crosses the host, so nothing the user sees can
differ unnoticed.

### 5.3 Replay At A Different Code Version

A rebuild after a Stop or a page reload runs the same modules, so a
divergence means a bug or a limit (7.3). `:reload` and share links from
another toolchain changed the code on request, so replay runs with
divergence detection and keeps the longest matching prefix. A whole-run
replay or resume requires the same program bytes.

### 5.4 Rebuilding A REPL Session

```text
rebuild(session, base'):                     ;; base' = the package snapshot to use
    new Store (or new program worker); instantiate prelude
    for each input i in order:
        if input i ended Stopped: skip it, note "input i was stopped; its changes are not restored"
        check input i against base' and the kept inputs   ;; cache hit when nothing changed
        if errors: drop inputs i.. ; report the first diagnostic; stop
        link and run input i under the Replayer (silent)
        if divergence or a different End: drop inputs i.. ; report; stop
    switch the providers to live
```

The rebuild keeps the longest prefix that checks and replays identically;
dropped inputs stay in the history for the user to enter again. Unlike
GHCi's `:reload`, bindings whose inputs still check and replay survive.
Same-code rebuilds recompile nothing. The CLI also rebuilds past 1000
inputs, holding each run of joined inputs in one module to bound memory
([REPL Decisions](#repl-decisions-2026-10-12-and-2026-10-13)).

## 7. Panics, Cancellation And Stops In The REPL

### 7.1 Panics

The REPL reports the panic and continues
([`cli.repl.panic`](../../spec/cli/command-line.md#r-cli.repl.panic)); the
input adds no binding or declaration; its changes to earlier values and its
host calls remain. wasmtime leaves the `Store` usable after a trap. The
input's own instance is poisoned
([`flow.panic.poison`](../../spec/lang/06-control-flow.md#r-flow.panic.poison)),
so its exports never enter `symbols`. Its journal rows stay, with
`End(panic)`, and a rebuild panics again at the same point.

### 7.2 Cancellation: Ctrl-C While Waiting

When the input is Pending, the host has control, so Ctrl-C or Stop calls
`hd.cancel()`, which cancels the root as
[`req.cancel.steps`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.steps)
describes. A `Cancel` row records it, and the input ends like a panic.

### 7.3 Stops: Ctrl-C While Running

A busy input is stopped by an epoch deadline on wasmtime (the `Store`
survives) or by terminating the worker in the browser after 250 ms (the
session is rebuilt). A stop happens at an arbitrary instruction that no row
marks, so a rebuild skips stopped inputs and says so; their changes are not
restored, and a later input that depended on them will most likely diverge
and be dropped. The `--time-limit` panic is the same kind of stop. Metering
loop headers so stops land on journaled host calls is Later; it is also
the fuel simulation testing needs.

### 7.4 Interaction Summary

| Input ends by | Binding added | Earlier changes | Host calls | Rebuild |
| --- | --- | --- | --- | --- |
| success | yes | yes | once | replayed |
| panic | no | remain | once | replayed; panics again |
| cancel (waiting) | no | remain | once; pending ones aborted | replayed, with the `Cancel` row |
| stop (busy), time limit | no | remain live; not restored by a rebuild | once | skipped |
| check error | no (never ran) | none | none | not journaled |

## 8. Browser And Playground

The playground keeps one program worker and the journal in IndexedDB.
Stop first sends `cancel`; if no answer comes in 250 ms, the page
terminates the worker, starts a new one, and rebuilds with the stopped
input skipped. A page reload restores the latest session by recompiling
(cache hits) and rebuilding. A share link carries the inputs, plus the
journal when it compresses to 16 KiB or less; with the journal the
receiver sees the sender's exact output, without it the receiver runs the
inputs live. A file without `main` runs as a script, as `hd FILE` does
([`module.init.script`](../../spec/lang/10-modules.md#r-module.init.script)).
The JS host's session keeps one module per input and has no 1000-input
bound.

## 9. Performance

A one-line input in a warm CLI session targets 30 ms or less: checking
against frozen interfaces, emitting a small module, Cranelift at
`OptLevel::None`, and instantiating with pre-resolved imports.

wasmtime frees no instance before its `Store`, so each kept input costs
memory until the session rebuilds. A kept input measures about 89 KiB, and
`long-session`'s bound is 100 KiB per input on 16 KiB pages, scaled by the
page size. Past 1000 inputs the CLI rebuilds into joined modules: 200
inputs rebuild in about 0.5 s, against 3.4 s with one module per input.
Replay costs only the program's CPU; a row append costs tens of
nanoseconds.

## REPL Decisions (2026-10-12 And 2026-10-13)

The owner's decisions on the REPL step plan of #17.

- Q1: bare `hd` on a terminal opens the REPL; piped text runs a program; no `hd repl`.
- Q2: an input's top level may make bang calls (a driver context, like `main!`'s body).
- Q3: a kept input is accepted under 100 KiB; `long-session`'s bound is 100 KiB on 16 KiB pages, scaled by the page size; past a limit the session rebuilds from the journal.
- Q4: the CLI REPL runs on wasmtime only.
- Q5: `rustyline` (Ctrl-C at the prompt) and `ctrlc` (SIGINT while an input runs, for Stop and Cancel).
- Q8a: inputs live outside every package folder. Q8c: the recorder sits at the host call sites. Q8e: the playground keeps one worker; Stop rebuilds from the journal.
- Q8b: **no redefinition.** An input must not redeclare a name an earlier input declared: `duplicate-type` for types, traits and aliases, `duplicate-binding` for functions and bindings (`cli.repl.redeclare.*`).
- Late `Debug` and `Display` are accepted. A session counts as one module, so an input may implement `Debug` or `Display` for an earlier input's type. An implementation added after a type's value was shown does not change the printer already built for it; a type not yet shown uses the new implementation.
- Derivation blocks and templates: a session counts as one module for `annot.block.module` and `annot.template.module`. An input may write `impl Debug for Point by Structure` for a type `src/lib.hd` or an earlier input declares. Member lines work the same way (#159); a type has one block with no trait in the whole session. A derivation built before a later block keeps the facts it saw.
- `:share` is browser-only and stays outside the spec.
- What a session builds (#159): an input builds only what its scope reaches, so a broken package file stops only the inputs that reach it. The driver builds, from the input's own module, the modules its uses reach and its session scope: the input before it, and `src/lib.hd` once the session holds it (`cli.repl.package.lib`, `cli.repl.uses.other`). `src/lib.hd` loads lazily, from the first input whose identifiers name one of the library's names, and stays held after that. With a broken `src/lib.hd`, `1 + 2` still runs, and `:reload` after the fix runs the library. `src/main.hd` and unreached modules are never read.
- Rebuild past a bound (#228):
  - **Trigger.** The CLI REPL rebuilds from its journal once more than 1000 inputs ran since it started or last rebuilt (`hd_run_wasmtime::REBUILD_AFTER`).
  - **Reporting.** No `reloaded:` line; stopped and dropped inputs are still reported (`cli.repl.rebuild.stopped`, `.prefix`).
  - **Linking.** Each run of consecutive inputs that joined the session is built as one module (`hd_driver::Packages::inputs_from`), which imports from the modules before it and exports every instance and global.
  - **Replay.** Input N runs from the joined module's `hd.init.N` and `hd.poll.N`, with one shared `hd.wake` and `hd.cancel`, under its own `Input` and `End` rows.
  - **Init order.** Each init runs in the input that first reaches its module, as it ran live.
  - **Code identity.** A joined input's `Input` row names the joined module's hash.
  - **Panics.** An input that panicked or exited is built alone and ends a run.
  - **Fallback.** When an input no longer builds or replays, the rebuild starts again with one module per input; `:reload` takes the same path.
  - **Website.** The JS host keeps one module per input and has no bound, so the web compiler leaves the joined build out (`JOINED_INPUTS`).

## Sources

- wasmtime `Store` (instances freed only with the store): <https://docs.wasmtime.dev/api/wasmtime/struct.Store.html>
- GHCi user's guide: <https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html>
- Julia 1.12 binding partitions: <https://julialang.org/blog/2025/10/julia-1.12-highlights/index.html>
- Jupyter reproducibility study: <https://leomurta.github.io/papers/pimentel2019a.pdf>
- Temporal activities and limits: <https://docs.temporal.io/activity-execution>, <https://docs.temporal.io/workflow-execution/limits>
- rr: <https://rr-project.org/>
- Elm, "The Perfect Bug Report": <https://elm-lang.org/news/the-perfect-bug-report>
