# hd-lang Runtime and Library Design

This document covers standard-library, tooling, and runtime facilities built on hd-lang's core language semantics. The normative syntax and language-level model live in the [formal specification](../spec/README.md); the [language tour](../guide/LANGUAGE_TOUR.md) is the readable introduction.

## Testing

A file keeps its unit tests in one `tests:` block. Each `it(...)` call in it
registers one test case:

```text
use std.testing.assert_equal

fn add(a: i32, b: i32) -> i32: a + b

tests:
    it("adds two values"):
        result := add(2, 3)
        assert_equal(
            result,
            5,
            reason="add should return the sum of both values",
        )
```

The language-level parts are normative in the specification. This document
describes the runner, the command line, and the testing library around them.

| Topic | Specification |
| --- | --- |
| The `tests:` block and its items | [Test Blocks](../spec/02-grammar.md#test-blocks) |
| `_test.hd` test modules, integration tests under `tests/`, test dependencies | [Test Modules](../spec/10-modules.md#test-modules) |
| `assert` and `assert_equal` | [Standard Testing](../spec/10-modules.md#standard-testing) |
| `it`, its options, and `it_each` | [Test Cases](../spec/10-modules.md#test-cases) |
| Instances, fakes, providers, pass and fail | [Test Outcomes](../spec/10-modules.md#test-outcomes) |
| A test body's result and `?` | [Propagation In Test Blocks](../spec/05-expressions.md#propagation-in-test-blocks) |

Assertions are ordinary functions from `std.testing`, not language syntax. Assertion functions require an explicit reason:

```text
assert(condition, reason="the condition should hold")
assert_equal(actual, expected, reason="both values should be equal")
```

Specialized functions such as `assert_equal` receive the actual and expected values directly, allowing structured failure diagnostics. The mandatory `reason` is a `string` expression recording the intended behavior.

### Test Runner

`hd test` makes a test build, which compiles the package's test code, and
runs every test case. The runner behaves as the
[testing redesign](TESTING.md#owner-decisions) decided:

| Behavior | Rule | Decision |
| --- | --- | --- |
| Isolation | Each test case runs in its own fresh program instance. | T21 |
| Parallelism | Test cases run in parallel by default. | T21 |
| Console | Each test case writes to its own `Console` buffer. The runner shows the buffer when the case fails. | T21 |
| Filesystem | Each test case gets a temporary filesystem, which the runner deletes afterwards. | T21 |
| Profile | A test run compiles against the `console` profile unless `hd test --profile NAME` selects another. | T20 |
| Unit test providers | A test case in a `tests:` block or a `_test.hd` module gets no host providers; its requirements come from `$.with` fakes. | T28 |
| Integration test providers | A test case under `tests/` gets real providers from the profile. One whose requirements the profile cannot bind is reported as skipped. | T20, T28 |
| Failed result | The runner prints an `.Err` result and its cause chain, as the host does for `main`. | T18 |
| Failed assertion | It panics with category `assertion-failed`. The runner reports the case as failed and runs the next one. | T19 |

A test case's id is `module::name`, and its name is any string literal
(T29). `hd test <text>` runs the test cases whose id contains the text, as
`cargo test` does (T10). `hd test --list` prints each id and its location
without running anything:

```console
$ hd test --list
billing::charges a fee after 30 days  src/billing.hd:14
billing::doubles[0]  src/billing.hd:20
```

An `it_each` call gives one test case per row, named `name[i]`. A loop
inside one `it` still works when one result for the whole table is enough
(T26, T31).

Hash values can shift when test code changes, because the `Hasher` seed
follows code identity (T26). A test that compares hash values sees that
shift.

### Snapshot Tests

A snapshot compares a string with expected text. The test picks the
rendering, such as `json.pretty(x)`, `yaml.encode(x)`, or `debug(x)`; there
is no strategy system (T32). `debug` renders the derivable `Debug` trait
(T33), which the specification does not have yet.

```text
use std.testing.{snapshot, snapshot_file}

fn greeting(name: string) -> string: "hello, " + name

tests:
    it("greets by name"):
        snapshot(greeting("Ada"), expect="hello, Ada")
        snapshot_file(greeting("Grace"))
```

`snapshot(text, expect="...")` keeps the expected text in the source (T30).
`snapshot_file(text)` takes no name; the runner names its file from the
test (T34):

| Part | Rule |
| --- | --- |
| Folder | One `__snapshots__/` folder at the package root, beside `hd.toml`. It has no `mod.hd`, so it is never a module. |
| Path | `__snapshots__/<module>/<test-slug>-<n>.snap`. A module under `tests/` appears as `tests.<name>`. |
| Slug | The test name, lowercased, with each run of non-alphanumeric characters turned into `-`. |
| Counter | `<n>` counts the `snapshot_file` calls within one test run, from 1. |
| Table rows | An `it_each` row adds its index: `<test-slug>.<i>-<n>.snap`. |

So the test above writes `__snapshots__/<module>/greets-by-name-1.snap`.
Renaming a test, or reordering its `snapshot_file` calls, changes the file
names; the owner accepted that cost.

| Command | Effect |
| --- | --- |
| `hd test --update` | Rewrites changed `expect=` literals, and writes new or changed snapshot files. |
| `hd test --review` | Shows each changed snapshot as a diff to accept or reject. It also lists snapshot files that no test wrote, for deletion. |

### Property Testing

Property testing is a library in `std.testing`, not language syntax (T12).
A property registers with `it_prop` or `it_prop_with`, which a `tests:` block
admits beside `it` ([Table Tests](../spec/10-modules.md#table-tests)):

```text
use std.testing.{assert, it_prop}

fn clamp(value: i32) -> i32:
    if value < 0: 0 else: value

tests:
    it_prop("clamp is never negative", fn!(value: i32):
        assert(clamp(value) >= 0, reason="clamp removes negatives")
    )
```

| Part | Decision | Behavior |
| --- | --- | --- |
| Generation | T35 | `@derive(Arbitrary)` derives a `build` over a recording `std.testing.Choices` source. Each member is generated by its type: lists draw a length, numbers lean toward 0, -1, and the extremes, and optionals and enum variants are picked. Member lines in a derivation block tune one member, as in `quantity = arbitrary.range(1, 99)`. Other constraints use a plain generator `fn(mut Choices) -> T` with `it_prop_with`. |
| Shrinking | T25, T35 | The runner replays smaller recorded choice streams through the same generator, in fresh program instances, as Hypothesis does. No type needs shrink code, constraints always hold, and a panic is just a failed run. |
| Registration | T36 | `it_prop(name, prop)` for `T < Arbitrary`, and `it_prop_with(name, gen, prop)`. Both are imported, not prelude names. A failure prints the shrunk value through `Debug` and the seed. |
| Regressions | T37 | The shrunk choice stream is saved under `__regressions__/<module>/<test-slug>` at the package root, next to `__snapshots__/`, and is replayed first on every run. |
| Budget | T38 | 100 cases per property by default, with sizes growing from small to large. `cases=` or `hd test --cases N` overrides it, and `hd test --seed N` reproduces a run. |

The draft signatures are:

```text
pub fn it_prop[T < Arbitrary](name: string, prop: fn!(T) -> void, cases: i32 = 100) -> void:
    pass

pub fn it_prop_with[T](name: string, gen: fn(mut Choices) -> T, prop: fn!(T) -> void, cases: i32 = 100) -> void:
    pass
```

`Arbitrary`'s members, the `Choices` API, and stateful testing are designed
with the standard library ([Testing Layer](STDLIB.md#testing-layer)).

## Capabilities and Sandbox

Capabilities use the ordinary dependency model. There is no separate capability declaration, type category, or signature syntax:

```text
trait FileRead:
    fn read!(self, path: string) -> Result[string, FileError]

fn load_config!(path: string) -> Result[string, FileError] $ FileRead:
    files := $.use(FileRead)
    files.read!(path)
```

`FileRead` is shown as the library declaration of an ordinary trait that the
example runtime profile designates as a host-bindable capability key.
Production, tests, and interactive sessions can provide different
implementations through the same context operations:

```text
$.with(FileRead=workspace_files):
    config := load_config!("config/app.json")?
```

```text
$.with(FileRead=memory_files):
    config := load_config!("config/app.json")?
```

A user-defined in-memory implementation can satisfy `FileRead` without receiving ambient filesystem access. If an implementation needs real filesystem, network, clock, secret, subprocess, or other host access, that access must itself come from the providers available to it.

hd-lang compiles to WebAssembly using Wasm GC for managed language values and a WASI-compatible host boundary. Authority-bearing providers originate at that boundary; a Wasm module cannot manufacture ambient filesystem, network, clock, randomness, secrets, or similar authority. User code may wrap or narrow an injected provider, and a test host may inject an in-memory implementation through the same dependency mechanism.

The runtime starts sandboxed and supplies no ungranted external-resource providers. Missing requirements remain compile-time errors at ordinary call sites, and an application or deployment entry point must have its full requirement row satisfied by its host configuration. Provider values can escape their original context through ordinary value flow, so the row is not by itself a complete authority-reachability report.

An entry point's transitive `$` requirements are the host provider-binding list:

```text
pub fn main!() -> Result[void, AppError] $ FileRead, Network:
    config := load_config!("config/app.json")?
    sync_config!(config)?
    .Ok()
```

The compiler derives and verifies that provider set from the entry point and everything it calls. Package and deployment manifests do not repeat a separate provider-binding list. Host configuration binds concrete providers and their scopes to the derived requirement keys. The official hd runtime implements every standard capability, but injects only the providers granted to a particular invocation. An alternate host may implement a subset. Running or deploying an entry point fails before execution when the selected host cannot bind every required provider. A host provider is used mutably exactly when its trait declares or inherits a `mut self` method, as the stateful services `Clock`, `Random`, `FsWrite`, and `Console` do; profiles mark nothing, and rows never write `mut` ([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)). Because provider values are ordinary values, this list is not a complete audit of authority that has escaped through value flow.

Every host-backed standard-library service is exposed as a trait requirement rather than a global API. `$`, `$.use`, `$.with`, and `$.Context[...]` are therefore the single mechanism for standard filesystem, network, clock, randomness, observability, workflow, and similar runtime services. Pure operations such as collection transforms, arithmetic, and in-memory parsing remain ordinary functions and require no context.

This design deliberately gives capabilities no special language semantics. Sandboxing is enforced by the runtime and host-provider boundary.

Decided 2026-09-27 (owner answers to this section's questions):

1. **Narrow capability traits, Deno style.** One trait per kind of
   authority, for example `FsRead`, `FsWrite`, `Net`, `Clock`, `Random`,
   `Env`, `Console`, and `Process`, so a read-only tool asks only for
   `$ FsRead`. Access follows from each trait's methods
   ([Mutable Providers](../spec/11-requirements-and-suspension.md#mutable-providers)).
   The exact list and method sets are still to be written.
2. **Grants: CLI flags and a manifest run profile, binding only.** Both bind
   each derived requirement key to a compatible host implementation, for
   example `hd run --grant FsRead=<host impl>` or an `hd.toml` run section.
   They do not carry scopes such as roots or host lists: the injected host
   implementation defines its own scope and configuration. The spelling is
   still to be written.
3. **Restrictions are attenuated provider values.** The host sets the
   starting scope; code narrows it further with ordinary library methods
   that return a new provider (object-capability style, like WASI
   preopens), for example `$.with(FsRead=fs.under("./data/users"))`. No
   language feature is needed.
4. **Resumption rebinds by key.** Providers are never serialized, because
   they are not boundary-safe. On resume the host rebinds each requirement
   key from the recorded provider configuration identity, and a mismatch
   is rejected ([Replay Rules](#replay-rules)).

5. **WASI: the component model; the release is chosen later.** The first
   runtime targets Wasm components. Whether it uses WASI 0.2 or 0.3 (native
   async, which fits suspension) is chosen when the runtime is built.

## Persistence and Resumption

A suspending function can be run as a durable workflow without adding checkpoint syntax:

```text
fn sync_user!(id: UserId) -> Result[void, SyncError] $ Database, RemoteApi:
    db, remote := $.use(Database, RemoteApi)
    user := db.load_user!(id)?
    remote.push_user!(user)?
    db.mark_synced!(id)?
    .Ok()
```

When a durable runner starts `sync_user!` with recording enabled, it records the entry function's stable identity, code identity, arguments, and provider configuration identity. It then runs the function normally. Calls that cross into the host append events to an append-only history; which calls are recorded, and how much each event holds, depend on the recording level (see Replay Rules below).

On replay, the runtime re-executes the entry function from the start:

1. A host call whose event has a recorded result receives that result; the host operation is not repeated. Providers written in hd are not intercepted: they re-execute, and only the host calls they make are served from the history.
2. A host call whose event was started but has no recorded result is handled by the in-flight policy in Replay Rules.
3. When execution reaches the end of the history, the instance switches to live execution and continues. Reaching the end of the history is not an exit.

The language guarantees that code between host calls is deterministic ([Runtime Boundary](../spec/11-requirements-and-suspension.md#runtime-boundary)): an instance's behavior depends only on its code identity, runtime profile, entry arguments, and the ordered host-call results and wake and cancellation events it receives, apart from the exceptions listed there. Time, randomness, and external reads are host calls, so their results enter the history like any other input. Which host calls the history holds, and how events are matched to calls, are decided in Replay Rules below.

### Replay Rules

These rules are decided. They bind every runtime that records or replays histories. The analysis behind them is in [Durable Replay](DURABLE_REPLAY.md).

- **Core and library split.** Durable replay is a runtime feature with a small specification and compiler contract. The specification owns the determinism clause; the compiler emits a semantic code identity; the runtime intercepts host calls, records executor scheduling events, detects divergence, and supplies idempotency keys. History storage, runners, retry, workflow APIs, and deployment routing are library work. There is no workflow keyword, no source label syntax, and no `Durable` trait.
- **Interception at the host boundary only.** The runtime intercepts calls where they cross into the host. It does not intercept calls to providers written in hd; those re-execute on replay.
- **Recording is opt-in.** A run records nothing unless its host or command line asks for recording. A REPL session and an ordinary `hd run` record no history. The recording level is chosen per run, never in source: none (the default), provider calls only, or everything. The levels differ only in output detail: at the provider-calls level an output records a fingerprint, and at the everything level it also records its full arguments; replay behaves the same at both ([Durable Replay decision 16](DURABLE_REPLAY.md#owner-decisions)). A run that records keeps every event, including in a run that panics and a run that never finishes: the history holds every event up to the panic, or up to the point where the host stops the run.
- **Code identity.** A history records one code identity: a hash of the semantic content of the entry module and all its transitive dependencies, together with the compiler's semantic version. It is not the hash of the Wasm binary. Any semantic change anywhere in that set, or a new compiler semantic version, invalidates every history recorded against it, and replay rejects such a history. Formatting and comment changes never change the code identity.
- **Runtime profile.** The runtime profile is part of the provider configuration identity. Replay under a different runtime profile is rejected. The host's stack and memory limits are part of the runtime profile, so a history never replays on a host with different limits ([`req.determinism.limits-profile`](../spec/11-requirements-and-suspension.md#r-req.determinism.limits-profile)). A limit failure itself is outside the replay guarantee: if replay hits a limit the recording did not, or misses one it did, replay reports a limit-divergence error ([Durable Replay decision 17](DURABLE_REPLAY.md#owner-decisions)).
- **Inputs and outputs.** The runtime profile marks each host method, suspending or not, as an input or an output. An input's result is recorded, and replay supplies it without the live call. An output records only a fingerprint, and replay suppresses the live call.
- **Event matching.** Replay matches each event to a call by its order, its provider and method key, and an argument fingerprint. Source site IDs from the compiler's site table appear only in diagnostic messages.
- **History values.** There is no `Durable` bound. Entry arguments and results, host payloads, and continue-as-new state are boundary-safe values ([Boundary-Safe Values](../spec/10-modules.md#boundary-safe-values)).
- **Hashing.** The standard `Hasher` is deterministic, seeded per code identity and runtime profile. Hash-flooding defense uses a keyed hasher that the program chooses explicitly ([`req.determinism.hash-seeded`](../spec/11-requirements-and-suspension.md#r-req.determinism.hash-seeded)).
- **Weak references and finalizers.** They exist only inside the standard runtime, and there are no user-visible finalizers ([`data.repr.runtime-only`](../spec/08-data-and-enums.md#r-data.repr.runtime-only)).
- **Observability hooks.** Observability and replay use separate hooks. Replay intercepts at the host boundary; observability uses compiler-generated adapters at registered boundaries plus host-boundary events. Both derive their IDs from the execution ID and the event index.
- **Panics.** A history records a panic by its specification diagnostic name, such as `suspension-invalid-state`, not by an exit status. The exit status stays a runtime-profile mapping.
- **Pinning and continue-as-new.** A run stays on the code artifact it started with. A long-running workflow reaches new code only through a library `continue_as_new`, which ends the run and hands boundary-safe state to a new run on the new artifact. There are no patch markers.
- **End of history.** Reaching the end of a history is not an exit. By default the instance resumes: it switches to live execution and makes its next host calls live. `defer` suites run only on a real exit or a real cancellation, never merely because the history ended. A host that wants to abort instead cancels the resumed instance, which then runs its cleanup live.
- **In-flight host calls.** A host call that was started but has no recorded result follows the policy that its runtime profile marks for that method. An idempotent method is re-run, and the runtime passes the host a stable idempotency key made of the execution ID and the event index. Any other method returns an `outcome-unknown` error to the program, which decides whether to check, compensate, or fail.

There is no `checkpoint` keyword. In hd-lang, the runtime does not serialize the active WebAssembly call stack. It reconstructs local state by replaying from the entry point and reusing recorded host-call results. Capability providers and live resource handles are not stored in workflow history; compatible providers are rebound when execution resumes. Serializable closures are not the primary workflow continuation mechanism, and their separate semantics remain in the backlog.

Interactive notebook-style sessions combine a live kernel with a deterministic execution journal. The journal exists only when the notebook host enables recording; a plain REPL session records nothing. While the kernel remains alive, closing and reconnecting a client reuses its current namespace without replay. Each successful cell atomically commits a run containing its cell and code identity, parent state, suspension events, state delta, and output.

If the kernel is lost, the runtime restores the latest serializable namespace snapshot and replays subsequent committed cell runs in their actual execution order. Recorded host-call results are reused, giving recovery the same deterministic boundary as durable workflows. Editing and rerunning an earlier cell starts a new history branch from that cell's parent state; runs descended from the previous version become stale. Resume reproduces an existing history, while rerun deliberately creates new computation.

## Observability

The runtime automatically observes semantic execution boundaries: application entry points, registered tool and RPC calls, workflow runs, interactive cell runs, suspending `!` operations, and runtime dependency/provider boundaries. It does not automatically create a span for every ordinary function call.

`Observability` is an explicit dependency. Compiler- or library-generated adapters around registered boundaries require it, while the wrapped business function keeps its own requirements:

```text
fn get_user!(id: UserId) -> Result[User, Error] $ Database:
    ...

# Illustrative generated adapter, not normative source syntax.
fn __tool_get_user!(id: UserId) -> Result[User, Error] $
    Database + Observability:
    # Open a tool span, run get_user!(id), and close it from the complete exit.
    ...
```

The generated adapter is the registered or deployed entry, so its transitive requirement graph exposes `Observability`. A user function that explicitly logs, creates a custom span, or records a metric also lists `Observability` directly and retrieves its provider through the normal context mechanism.

Every execution task carries execution-local observability state:

```text
ObservabilityContext:
    current_span
    log_fields
    span_links
    trace_context
```

Lexical scopes temporarily extend that state and restore the previous value on exit. Child tasks inherit a forked context, and suspension/resumption preserves it. A boundary adapter creates a span from the current parent, installs the new span as current, runs the operation, and ends the span from the operation's complete exit. This guarantees closure on success, failure, defect, cancellation, and interruption.

Explicit log records are automatically enriched with the current fields, operation identity, task identity, and error cause. When a current span exists, the same record is also added as a span event, so callers never pass trace or span IDs manually. A boundary's start and completion are represented by the span itself rather than duplicate start/end logs. Unhandled errors, defects, retries, cancellations, and failed suspensions produce automatic runtime events.

### Initial Provider Draft

The initial provider interface deliberately stays small:

```text
trait Observability:
    fn sample(self, candidate: SpanCandidate) -> bool
    fn emit(self, event: Observation) -> void
```

The runtime owns span IDs, current-span context, lifecycle, enrichment, and replay suppression. The provider chooses whether a candidate span is sampled and consumes normalized events:

```text
enum Observation:
    SpanStarted(event: SpanStarted)
    SpanEnded(event: SpanEnded)
    Log(event: LogRecord)
    Metric(event: MetricPoint)
    Runtime(event: RuntimeEvent)

enum Outcome:
    Succeeded
    Failed(error_type: string)
    Defect(error_type: string)
    Cancelled
    Interrupted

data SpanContext:
    trace_id: string
    span_id: string
    sampled: bool

data SpanCandidate:
    operation: OperationInfo
    parent: SpanContext?

data SpanStarted:
    context: SpanContext
    parent: SpanContext?
    operation: OperationInfo
    timestamp: Timestamp

data SpanEnded:
    context: SpanContext
    outcome: Outcome
    timestamp: Timestamp
    duration: Duration

data LogRecord:
    level: LogLevel
    message: string
    fields: Map[string, ObservationValue]
    span: SpanContext?
    operation: OperationInfo
    timestamp: Timestamp

enum ObservationValue:
    Bool(value: bool)
    Signed(value: i64)
    Unsigned(value: u64)
    Float(value: f64)
    String(value: string)
    List(values: List[ObservationValue])
```

`ObservationValue` is a standard tagged scalar representation. Telemetry does not implicitly serialize arbitrary application objects.

The compiler-generated boundary adapter conceptually performs these steps:

1. Resolve the explicit `Observability` provider.
2. Read the current parent span and compiler-generated `OperationInfo`.
3. Ask the provider whether to sample the candidate.
4. Create and install a runtime-owned `SpanContext`.
5. Emit `SpanStarted`, execute the wrapped operation, and retain its complete exit.
6. Restore the previous context and emit `SpanEnded` with the derived `Outcome`.
7. Return, fail, cancel, or interrupt exactly as the wrapped operation did.

At a declared boundary, returning `.Err(error)` maps to `Outcome.Failed` even though `Result` is returned through normal language control flow. Values are not captured by default; only the error type and explicitly supplied safe fields are recorded.

Standard-library logging helpers use the same provider:

```text
fn info(
    message: string,
    fields: Map[string, ObservationValue] = {},
) -> void $ Observability

fn process_user!(id: UserId) -> Result[void, ProcessError] $
    Database + Observability:
    log.info(
        "processing user",
        fields={
            "user.id": ObservationValue.String(string(id)),
        },
    )

    user := load_user!(id)?
    process!(user)
```

`log.info` emits one `Log` observation. The runtime adds scoped fields, operation/task identity, source information, and the current span. If a current span exists, the log is also represented as a span event by the exporter strategy.

### Provider Strategies

A development provider can format every event:

```text
data ConsoleObservability:
    output: TextOutput
    minimum_level: LogLevel

impl Observability for ConsoleObservability:
    fn sample(self, candidate: SpanCandidate) -> bool:
        true

    fn emit(self, event: Observation) -> void:
        self.output.write_line(format_observation(event))
```

A production provider can buffer events for OpenTelemetry export:

```text
data OTelObservability:
    queue: TelemetryQueue
    sampler: Sampler

impl Observability for OTelObservability:
    fn sample(self, candidate: SpanCandidate) -> bool:
        self.sampler.should_sample(candidate)

    fn emit(self, event: Observation) -> void:
        self.queue.try_push(event)
```

`emit` is non-suspending and best-effort. A runtime-managed worker batches and exports queued events. Export failure cannot alter application results; queue overflow and dropped-event counts are themselves runtime metrics. Reliable audit delivery remains a separate suspending dependency.

Tests can inject an in-memory recorder and assert normalized observations:

```text
tests:
    it("process_user records its log"):
        recording := RecordingObservability::new()

        $.with(Observability=recording):
            _ := process_user!("user-1")

        assert_equal(
            recording.log_messages(),
            ["processing user"],
            reason="processing should emit its structured log",
        )
```

Fan-out, filtering, redaction, and sampling are provider composition strategies rather than language syntax.

### Replay

Automatic observations receive stable identities derived from execution ID, boundary ID, attempt, and event kind. Deterministic replay does not re-emit observations for already completed history events. New workflow activations use new attempt identities, exporters may deduplicate by observation ID, and replay diagnostics use separate runtime events.

This provider API is an initial draft. Decided 2026-09-27: custom spans, metric instruments, sampling, and exporter configuration are library API with no syntax (for example `obs.span("load users"):` with a trailing block, and `obs.counter("users.loaded").add(n)`), designed with the standard library. Trace context is carried task-locally by the runtime ([Observability Hooks](OPEN_ISSUES.md#observability-hooks)). Privacy and redaction wait for `Secret[T]`.

## Resource Lifetime Backlog

hd-lang accepts block-scoped `defer` for synchronous cleanup on ordinary
control-flow exits and cancellation. Ownership-driven cleanup, alias-escape
prevention, automatic finalization, and policies for asynchronous or fallible
cleanup remain backlog work.

The design must preserve the distinction between two jobs. A resource protocol
attaches cleanup responsibility to a value and is visible to type checking and
tooling. A scope-exit action can capture arbitrary local state and handles
pragmatic cases such as restoring a temporary mutation, recording final metrics,
conditional cleanup registration, and commit-or-rollback. A protocol can model
the latter only through a general closure-backed guard or exit stack, while a
bare scope-exit statement cannot by itself prove that every resource is closed.

A later hybrid may combine protocol-owned cleanup for real resources with the
accepted block-scoped construct for ad hoc restoration. Unlike Go-style
function-scoped defer, registration in a loop is attached to that iteration's
body and runs before the next iteration begins.

The harder problem is alias escape. The current `mut` model controls write
permission, not ownership, lifetime, open/closed typestate, or cleanup
responsibility. The accepted scope-exit syntax still permits this conceptual
failure:

```text
let global_file: File? = .None

fn publish_file!() -> void:
    file := File::open("data.txt")
    defer:
        _ := file.close()
    global_file = file

# Later, after cleanup has closed the handle.
file := global_file?
match file.read():
    .Err(ResourceError.Disposed) => pass
    _ => panic("closed handle did not report Disposed")
```

The accepted Wasm-handle contract already requires an operation after close to
return `.Err(ResourceError.Disposed)` rather than trap. That checked failure is
not deterministic cleanup: lexical cleanup would run one action but would not
invalidate aliases stored in
globals, fields, containers, returns, or closures. Garbage collection also does
not provide prompt release. The leading candidate pairs `defer` with a
compiler-recognized `NonEscapable` locality category that propagates through
containers and captures. Other candidates include resource-only affine
ownership, scoped regions, typestate plus alias restrictions, or scoped
callbacks with non-escaping resource types. The resource design must also define cleanup
failure, `Result`/`?`, suspension, cancellation, replay, and whether live handles
may cross a durable suspension boundary.

## Serializable Closure Backlog

Serializable closures are a runtime goal, not accepted syntax or semantics.
Choosing an annotation, modifier, wrapper type, or inference rule comes only
after the representation contract is settled.

That contract must define:

1. snapshot versus preserved identity and aliasing for captures;
2. treatment of mutable captures, cycles, repeated references, trait values,
   nested closures, and erased or reified generic arguments;
3. stable code identity and compatibility across source, compiler, deployment,
   and runtime versions;
4. whether requirements, authorization, providers, and live resources are
   captured, rebound, or rejected;
5. compile-time rejection versus runtime serialization failure;
6. schema migration, sandbox validation, cancellation, expiry, delivery,
   idempotency, replay, and result compatibility.

Durable workflow resumption does not depend on this feature. Its accepted
initial model reconstructs execution through deterministic replay and recorded
host-call results rather than serializing a closure or active Wasm stack.

## Incremental Computation

Incremental computation should initially be a native-feeling
`std.incremental` library with runtime support, not a language keyword.
Decided 2026-09-27: the direction below is accepted as library work, and the
API is designed with the standard library; callback purity is documented,
not checked. It is
distinct from both a persistent function cache and durable replay:

- an incremental node recomputes when one of its tracked inputs changes;
- a cache reuses a result only while code, arguments, captures, providers, and
  external dependency identities remain valid; and
- replay restores the recorded result belonging to one historical execution,
  even if current external data has changed.

The computation callback is intended to be pure: a non-suspending `fn`, not
`fn!`, with no `$` requirements, mutable parameters, or mutable captures.
hd-lang does not track purity in function types or prove referential
transparency, and since `mut fn` was removed a function type does not say
whether a closure mutates its captures. The library cannot rely on the
compiler for this property through indirect calls.
Mutation of fresh, non-escaping local values remains permitted. A readonly
reference is not a snapshot or stable value: another mutable alias can change
what it observes between reads. Changing shared state must therefore enter
through a tracked input or a future stable-value constraint.

Illustrative library shape, not a final API:

```text
let price = incremental.input(100)
let quantity = incremental.input(2)

total := incremental.compute:
    price.get() * quantity.get()

view := incremental.observe(total)

incremental.update:
    price.set(120)
    quantity.set(3)

incremental.stabilize()
println(view.get())
```

The initial runtime direction is transactional updates, lazy demanded
recomputation in topological order, equality-based propagation cutoff, DAGs
with complete cycle diagnostics, whole-value tracking, and observation-driven
node lifetime. Dynamic dependencies are the tracked inputs actually read on a
successful evaluation; recomputation atomically replaces the old dependency
set.

External files, database reads, HTTP responses, clocks, environment values, and
other external inputs require stable revisions, snapshots, digests, ETags, or
equivalent dependency tokens. An opaque operation is volatile and prevents
persistent reuse. TTL and manual invalidation are freshness policies, not
substitutes for dependency correctness. Workflow replay never consults
incremental cache freshness, and invalidating a node never repeats an external
workflow action.

Persisted identities use `std.fingerprint` rather than `Hash`, which stays
process- and runtime-dependent. A fingerprint always records its algorithm and
version identifier. Several standard algorithms may exist, but one is the
mandatory default, and the evolution rules define how a runtime treats a
fingerprint from an older or unknown algorithm.

Human and AI tooling must be able to inspect node/code identity, source
location, value type, dependencies and dependents, external versions,
clean/dirty state, revision, cache state, duration, and invalidation cause.
Exact APIs, fingerprint protocols, storage tiers, distribution, collection
granularity, lifetime, and observability integration remain open.
