# New Compiler Design: Live Execution (REPL, Replay, Resume)

Part of the [compiler design](README.md).

Status: Design, not decided, 2026-10-07.

## Summary

The owner asked for a REPL that is not hacky, and for replay and resume
to reuse its mechanism. The orchestrator proposed one mechanism for all
three: an **execution journal** of every nondeterministic input. This
document tests that framing.

**Verdict.** The journal is the right mechanism for replay, for resume,
and for **rebuilding** a REPL session. It is the wrong mechanism for
**redefinition** in a REPL. So the REPL runs live, and the journal is a
by-product that lets it rebuild.

| Question | Answer |
| --- | --- |
| What is a session? | Its inputs (source text), a live program instance, and a journal |
| How does an input run? | Checked against the earlier inputs' interfaces, compiled to one small Wasm module, linked into the session's one `Store`, run once |
| Redefinition? | **Shadowing**, as OCaml, GHCi, Scala, Kotlin and Swift do. Earlier code and values keep the old item. The REPL prints a note naming the earlier items that still use it |
| What is journaled? | Host-call results, wake batches, cancellations, and an input marker per input. Nothing else is nondeterministic ([`req.determinism.depends`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends)) |
| Where is it recorded? | In the host, by a wrapper around the providers and the driver loop. **No compiled hooks are needed**, so the same debug or release binary can be recorded and replayed |
| When does the REPL replay? | Only to rebuild: a browser Stop of a busy input, a page reload, a share link, and `:reload` after the package's files change |
| What if a rebuild diverges? | The rebuild keeps the longest prefix of inputs that checks and replays identically. The first divergent input and every later one are dropped and reported. Host calls are never repeated live |
| First release | The REPL with shadowing, the in-memory journal, rebuild on Stop, reload and `:reload`, and playground persistence and share links |
| Later | `hd run --record` and `--replay` as public flags, resume (durable execution), hot reload, notebooks, simulation testing, deterministic stops by metering |

Three findings matter most:

1. **The prototype's REPL already is a replay design, done badly.** It
   reruns the whole session as one program for each input, with a host
   answer list (`src/repl.ts`). That costs O(n²) over a session. After a
   divergence it calls the host live, which repeats effects. The
   playground's Stop re-sends the inputs to a new worker with no
   answers at all, so host calls happen twice. The new design keeps the
   good part (a record of host answers) and drops the rest.
2. **Replay at a different code version is the hard part everywhere.**
   Temporal, Azure Durable Functions and Restate all need versioning
   APIs for it. A REPL redefinition is exactly a code change under
   replay. That is why redefinition must not go through replay.
3. **wasmtime never frees an instance before its `Store`.** A session of
   1,000 inputs keeps 1,000 instances. The `long-session` metric needs a
   precise meaning for the REPL (open question 2).

## 1. Prior Art

### 1.1 REPLs

| System | How an input runs | Redefinition | Known problems |
| --- | --- | --- | --- |
| GHCi | bytecode per input, linked with compiled modules; `:reload` recompiles changed modules | shadowing: a new `T` is unrelated to the old one | the old type prints as `Ghci1.T`, giving "expected `T`, found `T`" errors; `:reload` **discards every prompt binding** |
| OCaml toplevel | each phrase is type-checked and compiled (bytecode or native) into the running image | shadowing; old closures keep old definitions | same two-`t` confusion as GHCi; no way to update a function that others already call |
| Julia | JIT per method; top-level code runs in the latest "world" | methods are replaced; already-running code keeps its world age; structs can be redefined only since 1.12 (binding partitions) | world-age errors in long-running code; `invokelatest` needed for globals from a newer world; before 1.12, a struct change forced a restart |
| Revise.jl | watches files and re-evaluates changed methods | method replacement | could not handle struct changes before Julia 1.12; changes to macros and generated code need care |
| Scala 3, Kotlin | each input is wrapped in a generated object (`rs$line$N`) and compiled to JVM classes | shadowing through nested scopes | stale definitions are invisible; class loaders grow per input |
| Swift | the LLDB expression evaluator JIT-compiles each input into the process | shadowing | heavy; tied to LLDB |
| Clojure | each form compiles to a JVM class; functions are reached through **vars** | late binding: callers see the new function through the var | `defrecord` and protocol reloads create new classes; old instances stop satisfying `instance?` and the new protocol |
| Elixir, Erlang | modules are reloaded whole; two versions may live at once | fully qualified calls switch to the new version | loading a third version kills processes still in the oldest |
| evcxr (Rust) | each input compiles to a dylib loaded into a child process; variables live in a map of boxed values | shadowing | a crash loses all variables; panics lost variables until `preserve_vars_on_panic`, and it still had "variable has gone missing" bugs |
| Node, Deno, Chrome console | V8's `Runtime.evaluate` with `replMode` | `let`, `const` and `class` may be redeclared across inputs | special rules only for REPL scripts; redeclaring a page's `let` is still an error |
| Jupyter kernels | a long-lived interpreter; cells run in any order | late binding (Python globals) | hidden state: in one study only 4% of notebooks reproduced their output when rerun top to bottom; a kernel restart loses everything |

Two families appear:

- **Statically typed REPLs shadow** (GHCi, OCaml, Scala, Kotlin, Swift,
  evcxr). It is sound and cheap. Its cost is the "two `T`s" confusion
  and stale callers.
- **Dynamic REPLs late-bind** (Clojure, Python, Erlang, Julia for
  methods). Callers see fixes at once. Their cost is that data defined
  by the old code (records, structs, protocol instances) goes stale in
  ways the user cannot see.

No mainstream REPL rebuilds the session by replay on a redefinition.
The nearest is the reactive notebook (Pluto.jl, Observable, marimo),
which reruns dependent cells **live**, so it repeats their effects.

### 1.2 Durable Execution And Replay

| System | Mechanism | Known problems |
| --- | --- | --- |
| Temporal | the workflow's event history records each command and its result; a worker replays the history to rebuild state, then continues | code changes break replay with a **nondeterminism error**; fixes need `GetVersion`/`patched` markers or worker versioning; history is capped at 51,200 events or 50 MB, so long workflows must "continue as new" |
| Restate | a journal per invocation; `ctx.run` records a step's result; on recovery, completed steps return their recorded results | the same versioning problem; a step that crashed before its result was journaled runs again (at least once) |
| Azure Durable Functions | event sourcing; the orchestrator function replays from history at each step | strict code constraints (no clock, no I/O, no random in orchestrators); changing the order of activity calls breaks in-flight instances (`NonDeterministicOrchestrationException`); needs orchestration versioning |
| rr | records every input from the kernel (syscall results, signals) and runs threads on one core; replays exactly | 1.2x to 1.4x overhead; one core; tied to Linux and x86 or ARM performance counters |
| Elm debugger | records every message; replay reruns `update` over them; histories can be exported and imported ("the perfect bug report") | a changed `Model` type invalidates an imported history |

**Lessons for hd:**

1. Record at the boundary that is already the only source of
   nondeterminism. rr does it at syscalls, Elm at messages, Temporal at
   commands. hd has that boundary already: host imports and wakes.
2. Check the **request**, not only the result. Temporal and Azure detect
   divergence by comparing the command the code issues with the one in
   the history.
3. Replay at a new code version needs explicit versioning, or a rule
   that refuses it. Every durable system learned this.
4. Histories grow without bound unless something truncates them
   (Temporal's continue-as-new).

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

- The **inputs and the journal are the truth**. The live instance is a
  cache of what replaying them would build.
- The live instance is the fast path. Each input costs its own check,
  compile and run, never a rerun of earlier inputs.
- The session acts as code inside `src/lib.hd`
  ([`cli.repl.package.lib`](../../spec/cli/command-line.md#r-cli.repl.package.lib)),
  so it is one **program instance** in the spec's sense
  ([`module.init.program-instance`](../../spec/lang/10-modules.md#r-module.init.program-instance)).

### 2.2 Checking One Input

Each input is a synthetic file `repl#n` in folder `src`, checked as top-level
statements and declarations of a one-input module (commands.md §7.8).
The scope is a stack:

```text
lookup(name):
    input n-1's interface, then n-2's, ..., then input 1's     newest first: shadowing
    then the uses that earlier inputs declared
    then lib.hd's private scope, then std's prelude
```

- **Input interface.** After an input checks, its new names (items and
  top-level bindings with their types) are frozen into a small interface,
  as a folder interface is (resolution-and-interfaces.md §4.11). Later
  inputs read it and never re-check it. Types are fixed per input
  ([`cli.repl.input-types`](../../spec/cli/command-line.md#r-cli.repl.input-types)).
- **Declarations see earlier bindings.** A function in input 5 may read
  input 2's `log`. The prototype forbade this. The spec already allows
  it: the session is code inside `lib.hd`, and a module's top-level
  bindings are available to its functions
  ([`module.init.storage`](../../spec/lang/10-modules.md#r-module.init.storage)).
  Definite initialization holds by construction, because an earlier
  input's bindings have already run.
- **Input kinds.** A declaration input adds items. A statement input runs.
  An expression input runs and shows its value
  ([`cli.repl.value`](../../spec/cli/command-line.md#r-cli.repl.value)). An
  input may mix declarations and statements, as a script does.
- **Arenas.** Each input's compiler arenas are freed after it runs. Only
  interned names and types, and the frozen input interfaces, persist
  (commands.md §7.8 step 4).

```hd
hd> let prices: mut Map[string, u32] = Map.new()
hd> fn price(item: string) -> u32:          # reads input 1's binding
...     prices.get(item).unwrap_or(+0)
hd> prices.insert("tea", +3)
hd> price("tea")
3 : u32
```

### 2.3 Compiling And Linking One Input

**One Wasm module per input, in one `Store`.** This is what
commands.md §20.5 already sketches. This design fills in the linker.

1. **Roots.** The input's top-level statements form its init body
   (codegen.md §13.1, "REPL input").
2. **Collect.** Walk the reachable instances, as for a program. Each
   instance key (stable path plus type arguments) is looked up in the
   session's `symbols` table. A hit becomes a Wasm **import** from the
   module that defined it. A miss is emitted into this input's module.
3. **Storage.** Module storage (the globals of std, of package modules,
   and of earlier inputs' top-level bindings) exists once per session.
   The first input that reaches a module emits its storage globals and
   runs its init group, as
   [`module.init.group.once`](../../spec/lang/10-modules.md#r-module.init.group.once)
   asks. Later inputs import those globals.
4. **Exports.** The module exports every instance and global it defines,
   under its instance key. The session records them in `symbols`.
5. **Run.** Instantiate (imports pre-resolved by name), then drive the
   input's root through `hd.poll` and `hd.wake` as an entry (§14.4 of
   suspension.md). `RuntimeMeta.entry` is `ReplInput`.

```text
prelude.wasm      cached per toolchain: runtime globals, panic globals, wake table,
                  common std instances (dbg_text for primitives, List and string formatting)
repl#1.wasm       imports prelude;          defines global prices, exports it
repl#2.wasm       imports prices;           defines price, exports it
repl#3.wasm       imports prices, Map.insert instance
repl#4.wasm       imports price, dbg_text[u32]
```

- **Wasm GC types canonicalize across modules** (iso-recursive rec
  groups), so a value built in input 1 flows into input 4 unchanged.
  The emitter's canonical type order (wasm-layout.md §15.3) makes the
  same hd layout give the same rec group in every module.
- **Runtime type ids** (for `dyn Any` and type witnesses,
  wasm-layout.md §15.2) are assigned session-wide and append-only, so a
  shadowed type and its replacement get different ids.
- **Direct imports, not the hot-reload table.** With shadowing, a call
  always goes to one fixed item, so a direct `call` to an imported
  function is enough. The funcref table of engines-and-test-runner.md
  §18.5 stays off. It is only needed for late binding (§3.3).
- **Cache.** An input module's `code` and `link` entries are keyed by its
  TIR hash plus the hashes of the symbols it imports. A rebuild of the
  same inputs (§5.4) recompiles nothing.
- **The prelude** removes most of the first input's latency: without it,
  the first `1 + 2` would emit and compile the formatting code of std.

## 3. Redefinition Semantics

### 3.1 The Options

| Option | What `fn f` entered again does | Values built by old code | Cost | Precedent |
| --- | --- | --- | --- | --- |
| A. Shadowing | a new item `f`; later inputs see it; earlier items and closures keep the old `f` | unchanged, keep their old types | none: it falls out of the scope stack | GHCi, OCaml, Scala, Kotlin, Swift, evcxr |
| B. Late binding | the `f` slot in a funcref table is overwritten when the signature is unchanged; otherwise shadow | unchanged | one `call_indirect` per session call; a second rule for changed signatures | Clojure vars, Julia methods, the hot-reload plan |
| C. World age | new code sees new `f`; code already running keeps the old world until it returns to the top | unchanged | world counters; confusing errors | Julia |
| D. Replace and replay | recompile every input with the new `f` and rebuild the session from the journal | rebuilt as if the new `f` had always existed | O(session) per redefinition; diverges whenever the change alters a host call | none for REPLs; notebooks (live rerun) and Elm's code swap |

### 3.2 Why Not Replace And Replay

Option D is what the framing proposed. It fails on five counts:

1. **Earlier outputs silently change.** Input 3 showed `total = 12`.
   After `price` is redefined, the rebuilt session holds a different
   `total`, but the screen still shows 12.
2. **It diverges whenever the change matters.** If the new `f` reads a
   file the old one did not, every input that called `f` makes a
   different host call. The rebuild must stop there and drop every later
   input. A redefinition would destroy the session's tail. Going live
   instead would repeat effects, which breaks
   [`cli.repl.host.once`](../../spec/cli/command-line.md#r-cli.repl.host.once).
3. **A type redefinition can make earlier inputs ill-typed.** Redefining
   `data Point` with other fields makes input 4's `Point(x: 1, y: 2)`
   an error. The rebuild stops there too.
4. **Cost.** Every redefinition reruns the CPU work of every input. A
   session with one slow input pays for it again on each fix.
5. **It contradicts the spec.** "A later input never changes the type of
   an earlier input's binding"
   ([`cli.repl.input-types`](../../spec/cli/command-line.md#r-cli.repl.input-types)).
   Option D changes earlier bindings' values and possibly their types.

Replay is still right for a **rebuild of the same code** (a lost worker,
a page reload) and acceptable for a **deliberate code change the user
asked for** (`:reload` of package files, §5.4). There, divergence is
reported, not hidden.

### 3.3 Recommendation: Shadowing, With A Note

Shadowing (option A), for every kind of declaration:

| Redefined | Effect |
| --- | --- |
| a function | later inputs call the new one; earlier functions and closures keep the old one |
| a `data` or `enum` type | a new nominal type; old values keep the old type; the old one prints with the input that declared it |
| a top-level binding (`x := ...` again) | a new binding; the old storage stays for the functions that read it |
| a trait | a new trait; old impls implement the old one |
| an `impl` that overlaps an earlier input's impl for the same, unshadowed type | the ordinary overlap error, as in a module; redefine the type to start over |

Why shadowing:

- It is sound with no new rule. Every earlier item was checked against
  exactly the items it still calls.
- It matches the spec's fixed types per input and the host-once rule.
- It is what every statically typed REPL ships ("prefer proven simple
  models").
- It costs nothing: the scope stack already gives it.

Its known problem is stale callers. The REPL answers it with a **note**,
computed from the call edges in the inputs' fact records:

```text
hd> fn price(item: string) -> u32: prices.get(item).unwrap_or(+1)
note: total, checkout (inputs 5 and 7) still call the earlier price; enter them again to use this one
```

And the two-`T`s problem with a printed origin:

```text
hd> p
Point { x: 1, y: 2 } : Point (input 3)
error[type-mismatch]: expected `Point`, found `Point (input 3)`
  note: input 9 redefined `Point`; values made before it keep the earlier type
```

**Why not late binding (B) for the first release.** It needs the
funcref table on in every session build, plus a rule for a changed
signature (shadow? reject?). That is two semantics for one action. It is
also half of hot reload. If the owner wants callers to see fixes, B is
the Later step, on the table that is already reserved (open question 1).

**Impls are never replaced.** Two live impls of `Hash` for one type
would let one `Map` hash a key two ways. So an overlapping impl stays an
error, and the fix is to redefine the type.

**Memory of shadowed bindings.** A shadowed binding that no function
reads (a static fact: binding reads are direct global accesses) has its
global cleared, so `big := load()` entered ten times keeps one value
alive, not ten. Clearing is not observable
([`req.determinism.weak`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.weak)).

## 4. The Journal

### 4.1 Testing The Framing

The framing's premise holds. By
[`req.determinism.depends`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends),
an instance's behavior depends only on its compiled program, its
profile, its entry arguments, and the ordered sequence of host-call
results, wakes and cancellations. Every one of those crosses the host
API:

| Source | Crosses at |
| --- | --- |
| file, network, process I/O | provider `call` (`.start` and `.finish` for waits) |
| clock readings | the `hd:Clock` scalar imports |
| random draws | the `hd:Random` imports |
| scheduling | `hd.wake(n)` batches between polls, and `hd:rt/block` results |
| cancellation from outside | the driver cancelling the root |
| environment, arguments | `Env` and `Args` provider calls |

Code between host calls has no other source of nondeterminism
([`req.determinism.no-other-source`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.no-other-source)).
A NaN's payload is canonical before hashing, display and any boundary
([`types.display.nan-canonical`](../../spec/lang/04-type-system.md#r-types.display.nan-canonical)),
so engine NaN differences cannot reach the host.

**The simplification the framing missed:** because all of this crosses
the embedding API (runtime-and-host.md §17.9), the recorder is a host
wrapper. It needs **no `hd:hook` imports and no special build**. A
release binary recorded in production replays bit for bit with the same
bytes. The reserved hook points (suspension.md §14.7) stay for tracing
and for exploring interleavings below the wake level.

```rust
struct Recorder<P: Provider> { inner: P, journal: JournalWriter }
impl<P: Provider> Provider for Recorder<P> {
    fn call(&self, m: MethodId, args: &[u8], out: &mut Vec<u8>, cx: &mut CallCx) -> CallResult {
        let r = self.inner.call(m, args, out, cx);
        self.journal.call(m, digest(args), &r, out);          // one row
        r
    }
}
struct Replayer { journal: JournalReader, live: Option<ProviderSet> }   // live = None: pure replay
```

The driver loop (suspension.md §14.4) writes one `Wake` row per
`hd.wake` call, and `hd:rt/block` one `Block` row.

### 4.2 What Is Recorded

| Row | Written when | Payload |
| --- | --- | --- |
| `Header` | once | format number, execution id, code identity (§4.4), toolchain, profile, grant, limits, entry arguments, working directory |
| `Input` | a REPL input starts | input index, source hash, module hash |
| `Call` | a provider method returns | method id, request digest, `Ready(result bytes)` or `Pending(handle)` |
| `Finish` | a `.finish(h)` returns | handle, result bytes |
| `Wake` | the driver calls `hd.wake` | the poll count, the batch of handles |
| `Block` | `hd:rt/block` returns | the batch of handles |
| `Abort` | the program aborts a host operation | handle (checked on replay, not served) |
| `Cancel` | the host cancels the root (Ctrl-C while waiting, §7.2) | the poll count |
| `End` | an input or the run ends | outcome: value shown, exit status, panic category, cancelled, or stopped |

- **Scalars** (a clock reading, a random `u64`) are stored as their
  exchange encoding: LEB128, or raw IEEE 754 bits for floats. A clock
  call is about 12 bytes.
- **Structured results** are stored as the bytes the host wrote into the
  exchange buffer (runtime-and-host.md §17.4). The journal adds no
  encoding of its own.
- **Request digests.** A 64-bit hash of the method id and the argument
  bytes. Arguments of 8 bytes or less are stored as is. Replay compares
  the digest, as Temporal compares commands (§5.2).
- **Output methods** (`Console` writes, `hd:rt/stderr`) are recorded
  with a digest of their text. The text itself is not stored: replay
  produces it again.

### 4.3 Format

```text
journal = Header Row*
Row     = kind: u8, index: uleb, len: uleb, payload: [u8; len], crc32: u32
```

- **Append-only.** Each row carries its event index and a checksum. A
  torn tail after a crash is detected and cut at the last valid row.
- **Large payloads** (over 4 KiB) go to a blob store by content hash.
  The row holds the hash. Reading the same file twice stores it once.
- **No text format.** A debug dump (`hd journal show FILE`, Later)
  prints rows with method names from the ABI table.

### 4.4 Keys And Identity

| Key | Value |
| --- | --- |
| execution id | 128 random bits, chosen at the start of a run or a REPL session; recorded in the header |
| event index | the row's position, from 0, in that execution |
| code identity (`hd run`) | the hash of the program's Wasm bytes; two builds are the same program when their bytes are identical ([`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program)) |
| code identity (REPL) | per input, the hash of that input's module, in its `Input` row |

### 4.5 Where It Lives

| Context | Storage |
| --- | --- |
| `hd repl` | memory, for the session's life (first release); `build/.hd/repl/` for saved sessions (Later) |
| playground and website REPL | IndexedDB, written after each input (§8) |
| `hd run --record FILE` (Later) | the named file; blobs beside it |
| durable resume (Later) | a file written ahead of each effect (§6.3) |

### 4.6 Size And Growth

- The journal grows with the bytes the host delivers, plus about 10 to
  20 bytes per event.
- A typical REPL session (hundreds of inputs, a few file reads) is under
  1 MB. A loop that reads the clock a million times adds about 12 MB.
- **Cap.** Each session has a journal cap (64 MiB on the CLI, 32 MiB in
  the browser). Past it, recording stops and the REPL says once that the
  session can no longer be rebuilt. The live session continues. This
  degrades the extra, not the REPL.
- Durable runs face Temporal's problem. Their answer, restarting with
  explicit state, is an hd program pattern, not a runtime feature (§6.4).

## 5. Replay

### 5.1 Deterministic Re-Execution

The `Replayer` stands in for the providers:

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
end of journal:    pure replay: error "journal ended"; rebuild or resume: switch to live providers
```

- **Time is compressed.** A replayed timer or HTTP wait completes at
  once, so replay costs only the program's own CPU.
- **Output.** Two modes: **silent** (a REPL rebuild; nothing is printed
  again) and **echo** (`hd run --replay`; console and stderr text is
  printed again, every other effect is served).
- **Panics replay.** A panic is a deterministic point in the program, so
  a replayed input panics at the same place, with the same partial
  mutations.

### 5.2 Divergence

A divergence is a request that does not match the next row: another
method, other arguments, a poll where the journal has a wake, or a run
that ends before its `End` row.

```text
error[replay-divergence]: input 4 called Clock.now_ms, but the record has FsRead.read_text("prices.csv")
  --> repl#4:2:5
note: the session kept inputs 1 to 3; inputs 4 to 9 were dropped and are in the history
```

- The error names the event index, the expected and actual methods, and
  the hd location of the call (the host has the Wasm backtrace at the
  import, mapped through `hd.lines`).
- **Never go live after a divergence.** The prototype did, which repeats
  effects. A divergence ends the replay, and the context decides what
  happens next (§5.4).
- Divergence detection sees only what crosses the host. State that
  differs but is never observed is not detected until it is observed.
  Every output crosses the host, so nothing the user sees can differ
  unnoticed.

### 5.3 Replay At A Different Code Version

| Context | Rule |
| --- | --- |
| `hd run --replay` (Later) | the program's hash must equal the header's. Otherwise it refuses and names both hashes. A flag to replay anyway, with divergence detection, is Later |
| REPL rebuild after a Stop or a page reload | the same modules, so the same code; divergence means a bug or a limit (§7.3) |
| REPL `:reload` and share links from another toolchain | the code changed by request; replay runs with divergence detection and keeps the longest matching prefix |
| resume (Later) | the same hash, or refuse; versioning markers in the style of Temporal's `patched` are Later still |

### 5.4 Rebuilding A REPL Session

One algorithm serves every rebuild:

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

- **It keeps the longest prefix that checks and replays identically.**
  Dropped inputs stay in the history, so the user can enter them again
  live, by choice.
- **Triggers:** a browser Stop of a busy input (§8.2), a page reload, a
  share link, and **`:reload`** (CLI and website) after the package's
  files changed on disk.
- **`:reload` improves on GHCi.** GHCi drops every prompt binding on
  `:reload`. Here, bindings whose inputs still check and replay survive.
- **Same-code rebuilds recompile nothing.** In the browser the compiler
  worker keeps each input's `WebAssembly.Module`; in the CLI the `code`
  and `cwasm` entries hit.

## 6. Resume (Durable Execution, Later)

### 6.1 Model

```sh
hd run --journal run.hdj     # records durably while it runs
hd run --resume run.hdj      # after a crash: replay the journal, then continue live
```

Resume is `rebuild` for a program: the `Replayer` serves the recorded
prefix, and at the journal's end it switches to the live providers. This
is the Temporal, Restate and Azure Durable Functions model, with the
whole program as the workflow.

### 6.2 Code Identity

- Resume requires the same program hash as the header (§4.4). The
  compiler is deterministic, so rebuilding the same sources with the
  same toolchain gives the same hash.
- A changed program is refused in the first version of resume.
  Versioning (Temporal's `patched`, Azure's named versions) is a later
  design, if users need long-lived executions across deployments.

### 6.3 Durability

- Before a provider performs an effect, the recorder writes and syncs
  the `Call` intent; after it, the result. A crash between the two
  leaves an intent with no result.
- On resume, that one call is performed again live. So the call in
  flight at a crash runs **at least once**, as Restate's `ctx.run` and
  Temporal's activities do. Every other call runs exactly once.
- The cost is one sync per effecting call. Pure reads (clock, random)
  need no intent row.

### 6.4 What Cannot Be Resumed

A host resource that outlives one call lives outside both the program
and the journal: an open file handle, a socket, a listener, a child
process. After a crash it is gone, but the replayed program still holds
its handle.

**A simple rule for the first version of resume:** refuse to record
with `--journal`, and refuse to resume, a program whose **import list**
contains a method that opens such a handle. The import list is exact,
because only reachable host methods are imported
([`cli.cap.total.needs`](../../spec/cli/command-line.md#r-cli.cap.total.needs)),
so this is a static check before anything runs, like a total deny. It
needs one flag per method in the ABI table (`opens_handle`).

Programs that use only whole-value calls (read a file, write a file, an
HTTP request, the clock, random) resume. NonEscapable, after the first
release, can later name which handles are safe to reopen.

### 6.5 Checkpoints (Later Still)

Replay from the start costs the whole history. A checkpoint would save
the program's state. But neither wasmtime nor V8 can serialize a Wasm GC
heap, and the first release has no serializable closures. So the
pattern for long executions is the program's own: save explicit state
through a capability and start a new execution from it (Temporal's
continue-as-new).

## 7. Panics, Cancellation And Stops In The REPL

### 7.1 Panics

The spec rules exist: the REPL reports the panic and continues
([`cli.repl.panic`](../../spec/cli/command-line.md#r-cli.repl.panic)); the
input adds no binding
([`cli.repl.panic.binding`](../../spec/cli/command-line.md#r-cli.repl.panic.binding));
its changes to earlier values and its host calls remain
([`cli.repl.panic.mutations`](../../spec/cli/command-line.md#r-cli.repl.panic.mutations)).

| Concern | Design |
| --- | --- |
| the store after a trap | wasmtime leaves the `Store` usable after a trap; earlier instances keep their state. In the browser, a trap throws in the worker, which stays alive |
| the input's own instance | poisoned ([`flow.panic.poison`](../../spec/lang/06-control-flow.md#r-flow.panic.poison)): its exports are not added to `symbols`, so no later input links to it. Its declarations are dropped with its bindings |
| escaped values | a closure that the input stored into an earlier value before panicking still works. If it reads storage the input never initialized, that read panics in the calling input. This is artificial code, so no rule is added |
| the journal | the input's rows stay, with `End(panic, category)`. A rebuild replays it: it panics again at the same point and reproduces its mutations |

### 7.2 Cancellation: Ctrl-C While Waiting

When the input is Pending (waiting on a host operation), the host has
control, so Ctrl-C (or the playground's Stop) **cancels** the input's
root:

1. The driver calls a new export, `hd.cancel()`, which cancels the root
   suspension as [`req.cancel.steps`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.steps)
   describes: `defer` suites run, and external operations are aborted
   ([`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
2. The journal gets a `Cancel` row at that poll count. Cancellation
   deliveries are inputs in the determinism rule, so this replays
   exactly.
3. The input ends like a panic: no binding; its earlier changes remain.

### 7.3 Stops: Ctrl-C While Running

When the input is busy in Wasm (a loop), nothing returns to the host:

| Engine | Mechanism | Live state afterwards |
| --- | --- | --- |
| wasmtime | the epoch deadline is set to now, so the next epoch check traps | the `Store` survives; changes made so far remain, as after a panic |
| browser | the program worker gets no message while Wasm runs, so after a grace period (250 ms) the page terminates it | lost; the session is rebuilt (§5.4) |

A stopped input stopped at an **arbitrary instruction**, which no
journal row marks. Replaying it could loop forever. So a rebuild
**skips** stopped inputs and says so. Their host calls are not
repeated, and their changes to earlier values are not restored. A later
input that depended on those changes will most likely diverge, and the
rebuild drops it then. This is the same limit the spec already states
for host limits
([`req.determinism.limits`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.limits)).

The `--time-limit` panic
([`flow.panic.time-limit`](../../spec/lang/06-control-flow.md#r-flow.panic.time-limit))
is a stop of the same kind.

**A way to make stops deterministic (Later).** The emitter could meter
loop headers and function entries with a counter global and call a
host import every N units. A stop would then happen only at such a
call, which is a journal row, so stops would replay exactly, on every
engine. The same counter is the fuel that simulation testing needs.
It costs a decrement and a branch per loop iteration in session builds
(open question 4).

### 7.4 Interaction Summary

| Input ends by | Binding added | Earlier changes | Host calls | Rebuild |
| --- | --- | --- | --- | --- |
| success | yes | yes | once | replayed |
| panic | no | remain | once | replayed; panics again |
| cancel (waiting) | no | remain | once; pending ones aborted | replayed, with the `Cancel` row |
| stop (busy), time limit | no | remain live; not restored by a rebuild | once | skipped |
| check error | no (never ran) | none | none | not journaled |

## 8. Browser And Playground

### 8.1 The Same Model In A Worker

The playground's two workers (commands.md §7.9) map onto the session:

| Piece | Lives in |
| --- | --- |
| input interfaces, the `symbols` table, compiled `WebAssembly.Module`s | the compiler worker |
| instances, the heap, the `Recorder` in the generated JS glue | the program worker |
| inputs and the journal | IndexedDB, written by the page after each input's `End` row |

- The generated JS glue (runtime-and-host.md §17.10) wraps its provider
  interface with the same recorder, row format and replayer as the
  wasmtime host. One conformance test records on one engine and replays
  on the other.
- A `WebAssembly.Module` can be posted to another worker, so a rebuild
  re-instantiates modules without recompiling.

### 8.2 Stop

1. The page sends `cancel` to the program worker.
2. If the input was waiting, the worker runs `hd.cancel()` (§7.2). The
   session lives on.
3. If no answer comes in 250 ms, the input is busy. The page terminates
   the worker, starts a new one, and rebuilds (§5.4) with the stopped
   input skipped. The panel shows "session restored: 12 inputs, 40 ms".

This replaces the prototype's behaviour, which re-sent the accepted
inputs to a new worker and ran their host calls again.

### 8.3 Persistence

- **IndexedDB** holds, per session: the inputs, the journal rows, the
  blobs, the toolchain build id and the base snapshot key. The compiled
  modules live in the existing `MemoryStore` cache (cache.md §5.8).
- **A page reload** restores the latest session: recompile (cache hits),
  then rebuild.
- **Eviction.** The newest 10 sessions are kept, each under the 32 MiB
  journal cap.

### 8.4 Share Links

- A link carries the inputs, and the journal when it compresses to
  16 KiB or less. Otherwise it carries the inputs alone.
- **With the journal**, the receiver's page replays the session and
  shows the sender's exact output, as Elm's exported histories do. Then
  it continues live.
- **Without it**, the receiver runs the inputs live, which is a new
  session with the receiver's own clock, random and network.
- A link from another toolchain build replays with divergence detection
  and keeps the longest matching prefix (§5.3).

### 8.5 Playground Run Without `main`

The prototype runs a file without `main` with REPL semantics, silently.
That mixes two models. The spec already defines such a file: it is a
**script** ([`module.init.script`](../../spec/lang/10-modules.md#r-module.init.script)).
So Run executes it as `hd FILE` does: its top-level statements are the
entry behavior, and expression values are not printed. Inputs with
printed values belong in the REPL panel. "Open in REPL" can move a
script's statements there.

## 9. Performance

### 9.1 Input Latency

Target: a one-line input in a warm session shows its value in **30 ms or
less** on the CLI.

| Step | Estimate |
| --- | --- |
| check one input against frozen interfaces | 1 to 5 ms |
| collect, emit, link a small module | 1 to 3 ms |
| Cranelift at `OptLevel::None` for a few functions | 1 to 10 ms |
| instantiate with pre-resolved imports | under 0.1 ms |
| run, show the value | the input's own work |
| first input of a session | adds the prelude: one cached `cwasm` load, about 1 ms |

These are estimates. Slice 8's measurements fix them, as for
`unit-test-perf`. In the browser, V8's Liftoff compiles small modules in
well under a millisecond.

### 9.2 `long-session`

The metric asks for flat RSS over 1,000 REPL inputs (goals.md).

| Part | Growth |
| --- | --- |
| compiler arenas | none: freed per input |
| interners, input interfaces | grow with **new** names and types only |
| journal | grows with host bytes; capped (§4.6) |
| heap | shadowed bindings that no function reads are cleared (§3.3) |
| **wasmtime instances and code** | **one instance and one code mapping per input, never freed before the `Store`** |

The last row is the problem. wasmtime's documentation says a `Store`
frees no instance until it is dropped, and that a `Store` is unsuitable
for an unbounded number of instances. Each module's code is mapped
separately, so even a tiny input costs a page or more (16 KiB on Apple
silicon) plus its instance metadata: roughly 20 to 40 KiB per input, so
20 to 40 MB over 1,000 inputs.

Two answers:

1. **Bounded per input (recommended for the first release).** Define the
   REPL row of `long-session` as: the compiler's memory is flat, and the
   runtime grows by at most a fixed bound per input (for example 64 KiB)
   plus the session's own data. Measure it.
2. **Compaction (Later, or first release if the owner insists on strict
   flatness).** When the session passes N input modules (for example
   256), rebuild it into a new `Store` (§5.4) with the inputs merged
   into a few modules. Its cost is one replay of the session's CPU
   work, during a pause the REPL announces.

In the browser, V8 collects unreachable instances, but imports keep most
of them reachable, so the same analysis applies with smaller constants.

### 9.3 Replay Cost

- Replay costs the program's CPU only. Host waits are served at once,
  and payloads come from memory or IndexedDB.
- A typical playground session (50 small inputs) rebuilds in under
  100 ms, dominated by instantiation and the inputs' own work.
- A session with a slow input pays that input's CPU again on each
  rebuild. The panel shows progress, and the user can abandon the
  rebuild and start an empty session.

### 9.4 Recording Cost

One row per host call. A clock or random call already crosses into the
host, so a row append (tens of nanoseconds into a memory buffer) adds
little. Structured results are copied once more, from the exchange
buffer into the journal. No compiled code changes.

## 10. First Release And Later

| Feature | When | Notes |
| --- | --- | --- |
| per-input modules in one `Store`, the session linker, the prelude | first release | replaces the rerun-the-session prototype |
| input interfaces; declarations that see earlier bindings | first release | removes the prototype's restriction |
| shadowing with stale-caller notes and origin-marked types | first release | open question 1 |
| `Recorder` and `Replayer` in the wasmtime host and the JS glue | first release | internal; the REPL needs them |
| in-memory journal (CLI), IndexedDB journal (browser) | first release | |
| rebuild on browser Stop, page reload, share link and `:reload` | first release | longest-prefix rule |
| `hd.cancel()` for Ctrl-C while waiting | first release | also useful to `hd run` |
| playground: scripts run as `hd FILE` | first release | removes the silent REPL mode |
| share links with a small journal | first release | |
| `hd run --record` / `--replay` as public flags | Later (goals.md triage) | nearly free once the recorder exists (open question 3) |
| saved CLI sessions (`build/.hd/repl/`) | Later | the same journal on disk |
| resume (`--journal`, `--resume`) with the handle rule | Later | §6 |
| late binding through the funcref table; hot reload | Later | open question 1 |
| metered stops, simulation testing from a generated journal | Later | open question 4 |
| notebooks: rebuild up to the edited cell, then live | Later | the same rebuild algorithm |
| compaction of long sessions | Later, unless open question 2 says otherwise | |
| replay across code versions with markers | Later still | |

### 10.1 What The Framing Gets For Free Later

- **Hot reload.** Swap funcref table entries for unchanged signatures.
  For incompatible changes, the framing proposed restart and replay.
  That works for short sessions (a REPL, a frontend in development). It
  does not work for a server that has run for a day: its journal is
  large and its replay would diverge on the new code. There, a plain
  restart is the honest fallback.
- **Notebooks.** A cell edit rebuilds the session up to the edited cell
  from the journal, then runs the edited cell and the cells after it
  live. That is the rebuild algorithm plus a cut point.
- **Simulation testing.** A generated journal is a `Replayer` whose
  answers come from fakes and a seed instead of a file. The scheduler's
  freedom is the wake batch order, which the host owns. Interleavings
  below that level would use the reserved hooks.

## 11. Changes Needed Elsewhere

Listed, not made.

### 11.1 Spec

The REPL rows below were applied in spec pass S1e (owner, 2026-10-07),
as [Redefinition](../../spec/cli/command-line.md#redefinition),
[Interrupting An Input](../../spec/cli/command-line.md#interrupting-an-input)
and [Rebuilding A Session](../../spec/cli/command-line.md#rebuilding-a-session);
`cli.repl.panic.binding` was extended by a sibling rule,
`cli.repl.panic.declarations`. The running (Later) and Determinism rows
are not applied.

| Where | Change |
| --- | --- |
| `spec/cli/command-line.md`, REPL | new rule `cli.repl.declarations.bindings`: a declaration in an input may use the top-level bindings of earlier inputs |
| same | new rules `cli.repl.redefine.shadow` (a name declared again shadows the earlier one for later inputs; earlier items and values keep it) and `cli.repl.redefine.impl` (an impl that overlaps an earlier input's impl is the usual overlap error) |
| same | extend `cli.repl.panic.binding` to declarations: a panicking input adds no declaration either |
| same | new rules `cli.repl.cancel` (interrupting an input that waits cancels it by `req.cancel`; it then ends as a panic does) and `cli.repl.stop` (interrupting a running input ends it as a panic does) |
| same | new rules `cli.repl.rebuild` and `cli.repl.rebuild.prefix`: a session may be rebuilt by running its inputs again with host results served from its record; host calls are not repeated; an input that does not check or makes a different host call ends the rebuild, and it and later inputs are dropped and reported |
| same | new rule `cli.repl.rebuild.stopped`: a rebuild does not restore the changes of a stopped input |
| same | REPL commands (`:type`, `:reset`, `:reload`) stay out of the spec, as today |
| `spec/cli/command-line.md`, running (Later) | `cli.run.record`, `cli.run.replay`, `cli.run.replay.same-program`, and the `replay-divergence` error, when the flags ship |
| `spec/lang/11-requirements-and-suspension.md`, Determinism | extend `req.determinism.limits` to time limits and interruption, so a stop is outside the guarantee as stack and memory failures are |
| same | a note under `req.determinism.replay`: a replaying runtime checks each request against the record and stops at the first mismatch |
| `spec/lang/06-control-flow.md` | a note under `flow.panic.poison`: in the REPL the poisoned instance is the input's own, by `cli.repl.panic` |

### 11.2 Design Files

| File | Change |
| --- | --- |
| [commands.md](commands.md) §7.8 | the scope stack, input interfaces, declarations that see bindings, shadowing and the stale-caller note; link here |
| commands.md §20.5 | the session linker (`symbols`, imports by instance key, first-reacher owns storage), the prelude module, the recorder; mark "a panic in an input is open question 6" as answered by the spec and §7 here; add `:reload` |
| commands.md §7.9, §20.6 | REPL state split between the workers, IndexedDB sessions, Stop by cancel then terminate and rebuild |
| [engines-and-test-runner.md](engines-and-test-runner.md) §18.3 | a REPL `Store` lives for the session and uses on-demand allocation, not a pooling slot |
| engines-and-test-runner.md §18.4 | the browser runner's cancel message, grace period and rebuild |
| engines-and-test-runner.md §18.5 | the REPL does not need the hot-reload table in the first release; late binding would be its first user |
| [runtime-and-host.md](runtime-and-host.md) §16.4 | `EntryKind::ReplInput` already exists; add the export naming scheme for input modules |
| runtime-and-host.md §17.1 | per-method flags in the ABI table: `output` (echoed on replay) and, for resume Later, `opens_handle` |
| runtime-and-host.md §17.9 | `Recorder` and `Replayer` as `Provider` wrappers; the driver writes `Wake`, `Block` and `Cancel` rows |
| runtime-and-host.md §17.10 | the JS glue gets the same recorder, replayer and row format |
| [suspension.md](suspension.md) §14.4 | a third export, `hd.cancel()`, which cancels the root |
| suspension.md §14.7 | record and replay need no hooks; the hooks remain for tracing and interleaving exploration |
| [codegen.md](codegen.md) §13.1 | a REPL input's roots are its statements; collection consults the session's `symbols` and emits only misses |
| [cache.md](cache.md) | input module keys: TIR hash plus the hashes of imported symbols |
| [goals.md](goals.md) | the REPL row of `long-session`, after open question 2 |
| [open-questions.md](open-questions.md) | 23.1-6 (a panic in the REPL) points to the spec rules and §7 here |
| `website/playground/README.md` | a file without `main` runs as a script; Stop restores the session from the journal |

## 12. Open Questions For The Owner

1. **Redefinition: shadowing or late binding?** Shadowing keeps earlier
   functions calling the old definition, with a note that names them.
   Late binding makes callers see a fixed function at once when its
   signature is unchanged, at the cost of a funcref table call and a
   second rule for changed signatures. Replace-and-replay is ruled out
   (§3.2). **Recommendation:** shadowing in the first release; late
   binding Later, together with hot reload, on the reserved table.
2. **What does `long-session` mean for the REPL?** wasmtime keeps every
   input's instance until the session ends, so strict flatness needs
   compaction by rebuild. **Recommendation:** for the REPL, flat
   compiler memory plus a fixed bound per input (64 KiB), measured;
   compaction Later.
3. **Ship `hd run --record` and `--replay` in the first release?** The
   recorder and replayer exist for the REPL, so the flags add a file
   format, two rules and an error code. goals.md triaged them as Later,
   and new big features are on hold. **Recommendation:** keep them Later
   as public flags; use the mechanism internally (REPL rebuilds, and a
   cross-engine conformance test).
4. **May a rebuild lose a stopped input's changes?** The alternative is
   metered code, so stops happen only at journaled points and replay
   exactly. It costs a decrement and a branch per loop iteration in
   session builds, and it is also the fuel that simulation testing
   needs. **Recommendation:** accept the loss in the first release (a
   new spec rule says so, and the REPL reports it); metering Later, with
   simulation testing.

## Sources

- The prototype: `src/repl.ts` (`ReplSession.run`, its host answer list
  and divergence handling) and `website/playground/README.md` (REPL
  semantics without `main`; the Stop behaviour).
- wasmtime `Store` documentation, on instances not freed before the
  store: <https://docs.wasmtime.dev/api/wasmtime/struct.Store.html>
- A REPL on wasmtime, forum thread:
  <https://users.rust-lang.org/t/wasmtime-store-instance-module-repl/66435/2>
- GHCi user's guide (shadowing, `:reload` discarding bindings):
  <https://downloads.haskell.org/ghc/latest/docs/users_guide/ghci.html>
- Julia 1.12 highlights (binding partitions, struct redefinition):
  <https://julialang.org/blog/2025/10/julia-1.12-highlights/index.html>
- Binding partition behaviour changes:
  <https://discourse.julialang.org/t/behavior-changes-with-introduction-of-binding-partitions/128660>
- Revise.jl limitations:
  <https://github.com/timholy/Revise.jl/blob/master/docs/src/limitations.md>
- Scala REPL overview: <https://docs.scala-lang.org/overviews/repl/overview.html>
- Clojure `tools.namespace` (stale records and protocols after reload):
  <https://github.com/clojure/tools.namespace>
- Reloaded protocols: <https://nelsonmorris.net/2015/05/18/reloaded-protocol-and-no-implementation-of-method.html>
- Erlang code loading (two versions, purge):
  <https://www.erlang.org/doc/system/code_loading.html>
- evcxr release notes (panics and variables):
  <https://github.com/evcxr/evcxr/blob/main/RELEASE_NOTES.md>
- evcxr issue 92 ("variable has gone missing"):
  <https://github.com/evcxr/evcxr/issues/92>
- Chrome DevTools Protocol, `Runtime.evaluate` `replMode`:
  <https://chromedevtools.github.io/devtools-protocol/1-3/Runtime/>
- Jupyter reproducibility study (Pimentel et al.):
  <https://leomurta.github.io/papers/pimentel2019a.pdf>
- marimo, lessons from reinventing the notebook:
  <https://marimo.io/blog/lessons-learned>
- Temporal workflow limits: <https://docs.temporal.io/workflow-execution/limits>
- Temporal versioning (Go SDK): <https://docs.temporal.io/develop/go/workflows/versioning>
- Restate key concepts: <https://docs.restate.dev/foundations/key-concepts>
- Restate, building a durable execution engine:
  <https://restate.dev/blog/building-a-modern-durable-execution-engine-from-first-principles>
- Azure Durable Functions code constraints:
  <https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-code-constraints>
- Azure Durable Functions versioning:
  <https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-versioning>
- rr: <https://rr-project.org/> and "Engineering Record And Replay For
  Deployability": <https://arxiv.org/pdf/1705.05937>
- Elm, "The Perfect Bug Report": <https://elm-lang.org/news/the-perfect-bug-report>
