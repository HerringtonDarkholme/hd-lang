# Durable Replay: Core Or Library

Status: decision proposal for [Roadmap](ROADMAP.md#3-runtime-durable-replay)
area 3. Nothing here is accepted language behavior. Decided rules stay in
[Replay Rules](RUNTIME_AND_LIBRARY.md#replay-rules); open items stay in
[Open Issues](OPEN_ISSUES.md).

## Summary

Durable replay should be a **runtime feature with a small compiler and
specification contract**. It should not be a language feature with syntax, and
it cannot be a pure library.

- The language guarantees determinism by construction. It adds no checker and
  no syntax.
- The compiler emits a semantic code identity and a site table.
- The runtime intercepts calls at the host boundary and records scheduling
  events in the executor.
- Libraries own storage, runners, retry, workflow APIs, and version routing.

Hosts already see every external input, so the chosen line is Golem's. hd adds
compiler metadata that a pure Wasm host cannot recover.

## Owner Decisions

Decided 2026-09-26:

1. **Option B.** Durable replay is a runtime feature with a small
   specification and compiler contract; storage, runners, retry, and
   workflow APIs are library work.
2. **Question 1: host boundary only.** Calls are intercepted where they cross
   into the host. Providers written in hd re-execute on replay.
3. **Recording is opt-in.** A run records nothing unless the host or command
   line asks for it; a REPL session or an ordinary `hd run` records no
   history. The recording level is chosen per run, not in source: none
   (default), provider calls only, or everything. Question 2 still decides
   what each level records.
4. **Question 5: pin plus continue-as-new.** A run stays on the artifact it
   started with; a library `continue_as_new` hands state to a new run. No
   patch markers.
5. **Question 8: reaching the end of the log is not an exit.** Resuming
   (switching to live execution) is the default; `defer` runs only on a real
   exit or a real cancellation. A host that wants to abort instead cancels
   the resumed instance, which runs cleanup live. Still open: what happens to
   a host call that was in flight when the original run stopped (started,
   no result recorded), since not every call can be retried blindly.

## Problem

The accepted model re-executes a suspending entry point from the start and
feeds recorded results to its external calls
([Persistence and Resumption](RUNTIME_AND_LIBRARY.md#persistence-and-resumption)).
Four rules are already decided:

1. One code identity per module; any semantic change rejects old histories.
2. Every run records a history, including runs that panic or never finish.
3. Replay past the end of a history stops with a history-exhausted failure.
4. The runtime profile is part of the provider configuration identity.

Still open ([Replay Determinism](OPEN_ISSUES.md#replay-determinism-and-durable-workflows)):

- where calls are intercepted;
- how events are matched to calls;
- what makes code between calls deterministic;
- what values a history may hold.

The existing recommendation there is a compiler workflow mode with hooks at
every provider bang call. This document tests that recommendation against
other systems and against hd's own semantics.

## The Test

> Can a library get replay right without compiler or runtime support? What it
> cannot do is core.

"Right" means five things. Replay must reproduce every external input. It must
reproduce scheduling decisions. It must detect code or configuration drift.
It must stop cleanly at the end of a history. It must not repeat completed
external operations.

## What The Prototype Shows

The replay experiments in `src/compiler.ts` (`instantiate`) and
`test/fixtures/suspension/26-*.hd` and `32-*.hd` depend on compiler support in
three places:

- **Call-site offsets.** `src/emitter/function-body.ts` stores each host
  call's source offset in the global `$hd.host-call-site` before the call. The
  runner uses it to build `function:provider:Trait.method:offset` site IDs. No
  hd library can observe its own source offsets.
- **Function spans.** Per-function code identities hash HIR function spans.
  Only the compiler has these.
- **Scheduling.** `$runtime poll` events record whether each poll was pending.
  This is executor scheduling, below anything `Suspend[T]` lets user code touch.

The prototype also departs from the decided rules in two ways. Code identity is
per function, not per module. Site IDs contain byte offsets, so reformatting a
function changes them. Neither choice should carry forward.

## Research

### Evidence Table

| System | Where the line sits | How steps are identified | How code changes are handled | How determinism is enforced | What goes wrong in practice |
|---|---|---|---|---|---|
| **Temporal** | SDK library plus server. The SDK owns the scheduler: a custom event loop, or a V8 isolate in TypeScript. | Command order plus command type (activity, timer). No source IDs. | `patched`/`GetVersion` branches in code, or Worker Versioning, which pins runs to a build. | TypeScript isolate swaps in deterministic `Math.random`, `Date`, and timers, and removes `WeakRef` and `FinalizationRegistry`. Go has the static `workflowcheck` tool. Others rely on discipline and replay tests. | Deploys cause non-determinism errors, and affected workflow tasks retry until fixed. Patch branches pile up. History size limits force continue-as-new. |
| **Restate** | SDK library plus server that holds the journal. | Journal index plus entry type. | Immutable deployments. An invocation stays on the deployment it started on. Changing code in place breaks it. | Discipline plus `ctx.run` for side effects. A journal mismatch (RT0016) is detected at runtime. | Old deployments must stay alive until drained. Context calls inside `ctx.run` are a common error. |
| **Golem** | Wasm host. The worker executor records host and WASI calls in an oplog. No SDK is needed for durability. | Oplog position. | Automatic update replays the old oplog on new code and fails on divergence. Manual update uses save/load snapshot exports. | Guest code is Wasm; the host is the only source of nondeterminism. Divergence detection runs during replay. | [golem#3773](https://github.com/golemcloud/golem/issues/3773): long-lived agents stuck after both update modes. Host calls are at-least-once. Guest APIs grew anyway: atomic regions, persistence levels, idempotence mode. |
| **Azure Durable Functions** | Library (Durable Task Framework) plus storage. | Order of scheduled tasks. | Side-by-side deployment, now `defaultVersion` plus `versionMatchStrategy`. | Written code constraints, a Roslyn analyzer (C# only), and a partial runtime check (`NonDeterministicOrchestrationException`). | Reordering tasks breaks in-flight instances. Microsoft says the runtime check does not catch all violations. |
| **Cloudflare Workflows** | Platform runtime plus a `step.do(name, fn)` library API. | A user-written step name, used as a cache key. | The rules page reviewed here does not describe in-flight version handling. | A rules document only. | Non-deterministic step names re-run steps. Side effects outside steps repeat. `Promise.race` must be wrapped in a step. |
| **Unison** | Language and codebase manager. | No replay. Computations are serialized and moved instead. | Definitions are identified by hash; old hashes stay valid forever. | Abilities track effects in types. | Needs the codebase-as-database model. Not a replay design. |
| **Flix** | Language. | No durable replay. | Not applicable. | Purity and effects are tracked in types; pure code cannot perform effects. | Not applicable. The lesson is that determinism can be a type property. |
| **Koka** | Language. | No durable replay. | Not applicable. | Row-typed effects, including `ndet`; handlers may resume more than once. | Not applicable. The lesson is that a user handler can intercept every operation. |

Sources: Temporal
[versioning](https://docs.temporal.io/develop/typescript/workflows/versioning),
[TypeScript sandbox](https://docs.temporal.io/develop/typescript/workflows/basics),
[workflowcheck](https://pkg.go.dev/go.temporal.io/sdk/contrib/tools/workflowcheck);
Restate [versioning](https://docs.restate.dev/services/versioning); Golem
[durable computing](https://golem.cloud/blog/what-is-durable-computing/),
[persistence](https://learn.golem.cloud/operate/persistence),
[update failure](https://github.com/golemcloud/golem/issues/3773); Azure
[code constraints](https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-code-constraints),
[Roslyn analyzer](https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-roslyn-analyzer),
[orchestration versioning](https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-orchestration-versioning);
Cloudflare [rules of Workflows](https://developers.cloudflare.com/workflows/build/rules-of-workflows/);
Unison [big idea](https://www.unison-lang.org/docs/the-big-idea/); Flix
[effect system](https://doc.flix.dev/effect-system.html); Koka
[book](https://koka-lang.github.io/koka/doc/book.html).

### Patterns

1. **Every library solution also owns the scheduler.** Temporal and Restate
   work as libraries only because their host languages let a library supply
   futures, an event loop, or a sandbox. Temporal's Go SDK replaces `select`
   and goroutines with its own `Selector` and `workflow.Go`. hd seals
   `Suspend[T]`, restricts `PollContext` to `std.task`, and makes the
   combinators compiler intrinsics
   ([Cancellation](../spec/11-requirements-and-suspension.md#cancellation)).
   Owning the scheduler in hd therefore means being the runtime.
2. **Step identity is order plus kind, not source location.** Temporal,
   Restate, Durable Functions, and Golem all match by position and operation
   type. Only Cloudflare asks for names, and user-written names are a known
   source of bugs. Source site IDs help diagnostics, but correctness does not
   need them.
3. **Determinism is enforced where ambient authority exists.** Temporal needs a
   sandbox and Durable Functions needs an analyzer because `Date.now()` and
   threads are ambient in their host languages. Golem needs neither because
   Wasm has no ambient authority. hd is in Golem's position: every
   authority-bearing provider comes from the host boundary
   ([Wasm Boundary](../spec/10-modules.md#wasm-boundary)).
4. **Code change is the main cost in practice.** Every system has a versioning
   story, and every story has failure reports. Pinning runs to deployments
   (Restate, Temporal Worker Versioning, Azure's `Strict` strategy) is the
   simplest safe model. hd's decided whole-module identity already chooses it.
5. **Pure host durability still grows guest APIs.** Golem needed atomic regions,
   persistence levels, and idempotence controls. A guest-facing library layer
   is needed in any design.
6. **Effect-typed languages could do replay as a library handler.** Koka and
   Flix handlers intercept every operation, and their types prove that nothing
   escapes a handler. hd deliberately has neither. Providers are ordinary
   values that may escape, and purity is not tracked
   ([Provider Access](../spec/11-requirements-and-suspension.md#provider-access),
   [Incremental Computation](RUNTIME_AND_LIBRARY.md#incremental-computation)).
   The library-handler route is closed.

## Applying The Test To hd

| Obligation | Library alone? | Why | Owner |
|---|---|---|---|
| Capture every external input | Partly | A library can wrap providers with `$.with`, but it needs a hand-written wrapper per trait because hd has no reflection and derivation is still open. The host boundary sees every input with no per-trait code. | Runtime (host boundary) |
| Capture scheduling: readiness at each poll, `race!` winners, wake and cancel order | **No** | A replaying wrapper returns `Ready` on first poll, which changes which child `race!` picks. Reproducing "pending for three polls" needs a waker, and only `std.task` can reach one. | Runtime (executor) |
| Keep code between host calls deterministic | Cannot enforce, mostly not needed | Maps iterate in insertion order, NaN is canonicalized before hashing, boundaries, and `Display`, and there are no ambient globals. Remaining gaps: the standard `Hasher` seed ("not guaranteed stable across processes"), future weak references, and host resource limits. | Specification text |
| Detect code drift | **No** | A semantic hash needs the compiler front end, and a library cannot read its own program. | Compiler/toolchain |
| Detect configuration drift | **No** | Host configuration and the runtime profile are not visible inside the instance. | Runtime |
| Name sites for diagnostics | Only with user names | Source spans exist only in the compiler. With whole-module identity, histories never cross code versions, so order plus operation key is enough to match. Sites only improve messages. | Compiler (site table) |
| Serialize recorded values | Needs a new `Durable` bound | Host-boundary payloads are already boundary-safe values with a canonical encoding. Interception at hd provider calls would record arbitrary hd values instead. | Existing boundary rules |
| Idempotency keys | Yes, if given an execution ID | Execution ID plus event index. | Runtime |
| Storage, runner, retry, timers, workflow API, continue-as-new, deployment routing | Yes | Ordinary code over host capabilities. | Library |

The existing Open Issues option 1 intercepts "each provider bang call". That
set cannot be defined in hd. A provider value is an ordinary trait value: it
may be stored in a field, captured, or returned, and a call through it looks
like any other dynamic trait call. The host boundary is the only interception
line the specification already defines.

## Options

### A. Full Core

A workflow mode or annotation is added. The compiler inserts a hook at every
provider call and adds a `Durable` trait with derivation. A static determinism
checker runs, and optional source labels are allowed.

- For: the best diagnostics; semantic events such as `Database.load_user`.
- Against: provider calls cannot be defined, as shown above. A `Durable` trait
  would duplicate boundary-safe types. A determinism checker would need purity
  tracking, which hd rejected. Source labels exist to support patching, and
  whole-module identity rules patching out.

### B. Minimal Core Contract Plus Library (Recommended)

- **Specification:** a determinism clause, and the closing of the gaps listed
  above.
- **Compiler:** a semantic code identity plus a site table, emitted as Wasm
  custom sections.
- **Runtime:** interception of host imports; executor events for readiness,
  wake, and cancellation; divergence detection; history exhaustion.
- **Library:** `std.durable` and deployment tooling for everything else.

- For: every obligation the test marks core is covered, and nothing else. No
  syntax. The history format is the component-ABI encoding.
- Against: histories are low level (`Net.send`, not `Database.load_user`).
  Semantic grouping becomes an observability or library concern.

### C. Pure Library

A `std.durable` library offers wrapper providers and Cloudflare-style named
steps.

- For: no compiler work.
- Against: it fails the scheduling, code-identity, and configuration-identity
  obligations. User step names repeat Cloudflare's known failures.

### D. Wasm Host Only, Like Golem

The host records every import and uses the binary hash as code identity. The
compiler does nothing.

- For: no compiler work. It covers inputs, scheduling, and configuration.
- Against: a binary hash changes with debug info and compiler version, which
  breaks the decided "formatting never changes identity" rule. Divergence
  reports would name only import indexes. Golem's update failures show that a
  host-only design still needs guest-side controls.

B is D plus the two compiler artifacts and the specification clause.

## Recommendation

Adopt option B.

- **Core, in the specification.** Add a determinism clause. A program instance
  is a function of four inputs: its code identity, its runtime profile, its
  entry arguments, and the ordered sequence of host-call results and
  wake/cancel deliveries. The clause requires closing the `Hasher` seed gap
  (question 3) and constrains weak references (question 9). No syntax is added.
- **Core, in the compiler and toolchain.** Emit a semantic code identity as a
  `std.fingerprint` digest with its algorithm ID. Emit a site table that maps
  each host-call and bang-call site to a function and ordinal, for diagnostics.
- **Core, in the runtime.** Intercept host imports. Record executor readiness,
  wake, and cancellation events. Match events by order, provider and method
  key, and argument fingerprint. Stop with history-exhausted. Derive
  idempotency keys from the execution ID and event index.
- **Library.** History store trait, runner, retry and backoff policy, durable
  timers over `Clock`, continue-as-new, optional memoized steps, replay-test
  helpers in `std.testing`, and deployment pinning.

Once accepted, the Open Issues item narrows. "Explicit source labels" and
"`Durable` result serialization" drop out. "Interception at each provider bang
call" becomes "interception at the host boundary".

## Consequences

### Specification Chapter 11

Proposals only; this document edits no normative text.

- The [Runtime Boundary](../spec/11-requirements-and-suspension.md#runtime-boundary)
  sentence that calls durable replay a runtime or library concern stays true.
  It gains one normative dependency, the determinism clause, placed in this
  chapter or in [Modules](../spec/10-modules.md#wasm-boundary).
- History exhaustion is a new way for a run to stop. It is neither completion
  nor cancellation, so the chapter must say whether `defer` suites run
  (question 8).
- Cancellation becomes a recorded input. The existing ordering rules (children
  first, then frames innermost outward) are already deterministic, so no change
  is needed there.
- `std.task.all!` already polls children in argument order. That rule is part
  of what makes replay work and must not be relaxed.
- No `Durable` trait, no site-label syntax, and no workflow keyword.

### Standard Library

- `std.durable`: runner, `HistoryStore` requirement trait, continue-as-new, and
  replay diagnostics.
- `std.task` retry: backoff waits go through `Clock` host calls, so they are
  recorded like any other input.
- `std.testing`: run recorded histories against a new build in CI. This is
  Temporal's replayer lesson and the cheapest defense against drift.
- The host capability catalog (roadmap area 4) must classify each method as an
  input or an output for replay (question 2).

### Items Moved Into Area 3

Only where the replay decision changes them:

- **Observability hooks.** The premise of one shared interception point does not
  hold. Replay needs the host boundary. Useful spans sit at semantic boundaries
  such as registered tools and `Database.load_user`, which the host never sees.
  Keep the compiler-generated boundary adapters already designed in
  [Observability](RUNTIME_AND_LIBRARY.md#observability). Host-boundary events
  feed low-level spans. Both derive observation IDs from execution ID and event
  index, and replay suppression happens at the host hook (question 11). The
  task-local carrier in `PollContext` is unchanged.
- **Serializable closures and incremental computation.** Replay no longer
  depends on closure serialization, which was already true. The semantic hash
  built for module identity is the same machinery that per-definition closure
  identity (Unison-style) would need later, applied at a finer grain. The
  Open Issues order stays: registered computations first, content hashes after
  this decision.
- **Weak references and finalizers.** Collection timing is nondeterministic.
  Temporal removes `WeakRef` and `FinalizationRegistry` from its sandbox for
  this reason. User-visible finalizers conflict with the determinism clause.
  `std.incremental` should use observation-driven lifetime or explicit disposal
  rather than weak references.
- **Asynchronous and fallible cleanup.** Cleanup that makes host calls is
  recorded like any other code. The new constraint is history exhaustion:
  running cleanup there would make host calls past the history.
- **`std.fingerprint`.** Code identity becomes its first persisted user. The
  default algorithm and its version ID are now part of the history format, and
  changing either invalidates histories unless both digests are stored.
- **Runtime-profile panic codes.** Histories record panicking runs, so the
  terminal event needs a stable panic identity for comparison. Use the
  specification's diagnostic names (`suspension-invalid-state`), not exit
  statuses (question 12).
- **Wasm component ABI.** Under option B the history event format is the
  canonical-ABI encoding of host-call arguments and results. Choosing the ABI
  chooses the history format, and the prototype's type-tagged JSON is a
  stand-in. Readiness under WASI 0.3 async should be recorded at the
  waitable-set event level.

## Questions For The Owner

### 1. Where are calls intercepted?

- (a) At the host boundary only.
- (b) At every hd provider bang call.
- (c) Both.

**Recommendation: (a).** The set of calls in (b) cannot be defined, because
providers are ordinary values.

```text
trait Database:
    fn load_user!(self, id: UserId) -> Result[User, DbError]

data HttpDatabase:
    net: Net

impl Database for HttpDatabase:
    fn load_user!(self, id: UserId) -> Result[User, DbError]:
        body := self.net.get!(user_url(id))?   # recorded: Net.get
        parse_user(body)                       # re-executed on replay

fn sync!(id: UserId) -> Result[void, DbError] $ Database:
    db := $.use(Database)
    cached := db          # an ordinary value; no call is "the provider call"
    _ := cached.load_user!(id)?
    Ok()
```

### 2. Which host calls enter the history?

- (a) Suspending calls only.
- (b) Every host call.
- (c) Each host method is declared by its runtime profile as an **input**
  (result recorded and replayed) or an **output** (fingerprint recorded, live
  call suppressed on replay).

**Recommendation: (c).** Under (a), a non-suspending `Clock.now` would break
replay. Under (b), histories fill with telemetry payloads. With (c), output
suppression also gives observability its replay suppression.

```text
fn stamp!() -> Timestamp $ Clock + Observability:
    now := $.use(Clock).now()          # input: result recorded
    log.info("stamped", fields={})     # output: suppressed on replay
    now
```

### 3. Is the standard `Hasher` deterministic?

- (a) Seeded per process, as today.
- (b) Fixed within one code identity and runtime profile.
- (c) Seeded per execution, with the seed recorded as a history input.

**Recommendation: (b).** It closes the gap without a new event kind. Hashes may
still change across runtime versions, and those already invalidate histories.

```text
fn pick!(ids: list[UserId]) -> UserId $ Net:
    first := ids[0]
    if hash_of(first) % 2 == 0:     # under (a) replay may take the other branch
        ping!(first)
    first
```

### 4. What does code identity cover?

- (a) The entry module only.
- (b) The entry module, all transitive dependencies, and the compiler's
  semantic version.
- (c) The Wasm binary hash.

**Recommendation: (b).** A dependency update changes behavior as much as a local
edit. (c) breaks the decided formatting rule.

```text
use acme.pricing.total    # dependency update changes total()

pub fn main!() -> Result[void, AppError] $ Net:
    t := total(cart!()?)   # under (a) an old history silently replays new math
    charge!(t)
```

### 5. How do long-running workflows reach new code?

- (a) Pinned only: old runs drain on the old artifact.
- (b) Pinned, plus a library continue-as-new that restarts on new code with
  boundary-safe state.
- (c) Temporal-style patch markers.

**Recommendation: (b).** (c) contradicts the decided whole-module identity. (b)
also bounds history length.

```text
fn poll_forever!(state: PollState) -> Result[void, PollError] $ Net + Workflow:
    next := poll_once!(state)?
    if next.rounds % 1000 == 0:
        return $.use(Workflow).continue_as_new!(next)   # illustrative API
    poll_forever!(next)
```

### 6. How are events matched to calls?

- (a) Order plus provider and method key plus argument fingerprint, with site
  IDs used only in diagnostics.
- (b) Compiler site IDs.
- (c) User labels.

**Recommendation: (a).** Whole-module identity means histories never cross
edits, so labels have nothing left to protect. Drop "explicit source labels"
from Open Issues.

```text
fn transfer!(a: Account, b: Account) -> Result[void, BankError] $ Bank:
    bank := $.use(Bank)
    bank.debit!(a)?     # event 0: Bank.debit, fingerprint(a)
    bank.credit!(b)?    # event 1: Bank.credit, fingerprint(b)
    Ok()
# divergence message: "event 1 expected Bank.credit at transfer (bang #2)"
```

### 7. Is a `Durable` bound needed?

- (a) Add a `Durable` trait.
- (b) Reuse boundary-safe types for entry arguments and results, host payloads,
  and continue-as-new state.

**Recommendation: (b).** Revisit only for serializable closures.

```text
data PollState:          # boundary-safe: pub fields, structural, acyclic
    pub cursor: string
    pub rounds: i64

pub fn main!(state: PollState) -> Result[void, PollError] $ Net:
    ...
```

### 8. Does history exhaustion run `defer` suites?

- (a) No. The replayed run is paused, like abandoning a suspension that was
  never cancelled.
- (b) Yes, as if cancelled.

**Recommendation: (a).** Under (b), cleanup makes host calls past the end of the
history and leaves the run in a state the live run never reached.

```text
fn upload!(path: string) -> Result[void, IoError] $ Fs + Net:
    file := $.use(Fs).open!(path)?
    defer:
        _ := file.close()           # not run when replay stops early
    $.use(Net).put!(file.read!()?)  # history ends here during replay
```

### 9. May weak references or finalizers exist?

- (a) Never.
- (b) Only inside the standard runtime (for example `std.incremental`), with
  no clearing that user code can observe.
- (c) Allowed, with each read of a weak reference recorded as a host input.

**Recommendation: (b), with no user-visible finalizers.**

```text
let cache: WeakRef[Report] = WeakRef::empty()   # hypothetical type

fn report!() -> Report $ Net:
    match cache.get():              # under (c) this read must be recorded
        nil => fetch_report!()
        r => r
```

### 10. Are resource limits part of configuration identity?

- (a) Yes: stack and memory limits belong to the runtime profile.
- (b) No: a limit panic during replay is reported as divergence.

**Recommendation: (a).** Otherwise the same history replays differently on a
smaller host.

```text
fn depth(n: i64) -> i64:
    if n == 0:
        return 0
    1 + depth(n - 1)

pub fn main!() -> void $ Net:
    _ := depth(fetch_n!())   # stack overflow depends on host limits
```

### 11. Do observability and replay share one hook?

- (a) One hook at hd provider calls for both.
- (b) Replay at the host boundary. Observability uses compiler-generated
  adapters at registered boundaries plus host-boundary events. Both use
  execution ID and event index.

**Recommendation: (b).** (a) inherits question 1's problem.

```text
@tool   # illustrative registration annotation
fn get_user!(id: UserId) -> Result[User, Error] $ Database:
    $.use(Database).load_user!(id)
# span: tool get_user (adapter); child span: Net.get (host event)
```

### 12. How is a panic recorded in a history?

- (a) By the profile's exit status.
- (b) By the specification's diagnostic name, with the exit status kept as a
  profile mapping.

**Recommendation: (b).** Replay compares outcomes across hosts whose statuses
may differ.

```text
pub fn main!() -> void $ Net:
    let s: mut Suspend[string] = fetch_name("a")   # fn fetch_name!(key: string) -> string
    _ := s!()
    _ := s!()    # terminal event: panic suspension-invalid-state
```
