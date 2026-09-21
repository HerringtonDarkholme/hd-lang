# hd-lang Runtime and Library Design

This document covers standard-library, tooling, and runtime facilities built on hd-lang's core language semantics. The syntax and language-level model remain in the [language tour](LANGUAGE_TOUR.md).

## Testing

Unit tests use dedicated named blocks:

```text
import std.testing.{assert, assert_equal}

test "adds two values":
    result := add(2, 3)
    assert_equal(
        result,
        5,
        reason="add should return the sum of both values",
    )
```

A `test` block is a module-level test entry point discovered by the test runner. Its body uses normal hd-lang bindings, expressions, control flow, and function calls. It is not an annotation and does not need manual registration.

Assertions are ordinary functions from `std.testing`, not language syntax. Assertion functions require an explicit reason:

```text
assert(condition, reason="the condition should hold")
assert_equal(actual, expected, reason="both values should be equal")
```

Specialized functions such as `assert_equal` receive the actual and expected values directly, allowing structured failure diagnostics. The mandatory `reason` is a `string` expression recording the intended behavior.

Property testing is intentionally deferred because it has a much larger API and runtime surface than unit assertions. It must be implemented as a library-level facility in `std.testing`, not as dedicated property-test syntax in the language.

No property-testing API has been accepted yet. Strategy representation, generated-value access, type- and annotation-based derivation, dependency injection, shrinking, replay, correlated inputs, stateful testing, and failure artifacts all remain open. General language features such as reified generics, declaration shapes, annotations, and dependency contexts may support that library, but they should be designed independently rather than around one provisional property-testing API.

## Capabilities and Sandbox

Capabilities use the ordinary dependency model. There is no separate capability declaration, type category, or signature syntax:

```text
trait FileRead:
    fn read!(path: string) -> Result[string, FileError]

fn load_config!(path: string) -> Result[string, FileError] $ FileRead:
    files := $.use(FileRead)
    files.read!(path)
```

`FileRead` is an ordinary trait used as a requirement key. Production, tests, and interactive sessions can provide different implementations through the same context operations:

```text
$.with(FileRead=workspace_files):
    config := load_config!("config/app.json")?
```

```text
$.with(FileRead=memory_files):
    config := load_config!("config/app.json")?
```

A user-defined in-memory implementation can satisfy `FileRead` without receiving ambient filesystem access. If an implementation needs real filesystem, network, clock, secret, subprocess, or other host access, that access must itself come from the providers available to it.

hd-lang compiles to WebAssembly using Wasm GC for managed language values. WASI is the host boundary. Every authority-bearing capability provider originates at that boundary; a Wasm module cannot manufacture ambient filesystem, network, clock, randomness, secrets, or similar authority. User code may wrap or narrow an injected provider, and a test host may inject an in-memory implementation through the same dependency mechanism.

The runtime starts sandboxed and supplies no ungranted external-resource providers. Security follows dependency reachability: code can only reach authority exposed by its current provider context. Missing requirements remain compile-time errors at ordinary call sites, and an application or deployment entry point must have its full requirement row satisfied by its host configuration.

An entry point's transitive `$` requirements are the authoritative capability list:

```text
pub fn main!() -> void $ FileRead + Network:
    config := load_config!("config/app.json")?
    sync_config!(config)?
```

The compiler derives and verifies that provider set from the entry point and everything it calls. Package and deployment manifests do not repeat a separate capability permission list. Host configuration binds concrete providers and their scopes to the derived requirement keys. The official hd runtime implements every standard capability, but injects only the providers granted to a particular invocation. An alternate host may implement a subset. Running or deploying an entry point fails before execution when the selected host cannot bind every required provider.

Every host-backed standard-library service is exposed as a trait requirement rather than a global API. `$`, `$.use`, `$.with`, and `$.Context[...]` are therefore the single mechanism for standard filesystem, network, clock, randomness, observability, workflow, and similar runtime services. Pure operations such as collection transforms, arithmetic, and in-memory parsing remain ordinary functions and require no context.

This design deliberately gives capabilities no special language semantics. Sandboxing is enforced by the runtime and host-provider boundary.

Open questions from this section:

1. Which standard capability traits ship in v1.
2. Which WASI version and component ABI the initial runtime uses.
3. The configuration syntax for granting and binding host providers to derived entry-point requirements.
4. How path, host, secret-name, and subprocess restrictions are represented inside provider values.
5. How capability contexts are preserved or rejected during serialization and resumption.

The granularity of standard capability traits is intentionally deferred until the standard library is implemented. For example, the design does not yet choose between one `FileSystem` trait and narrower `FileRead`, `FileWrite`, and `DirectoryList` traits.

## Persistence and Resumption

A suspending function can be run as a durable workflow without adding checkpoint syntax:

```text
fn sync_user!(id: UserId) -> Result[void, SyncError] $ Database + RemoteApi:
    db, remote := $.use(Database, RemoteApi)
    user := db.load_user!(id)?
    remote.push_user!(user)?
    db.mark_synced!(id)?
```

When a durable runner starts `sync_user!`, it records the entry function's stable identity, code version, arguments, and provider configuration identity. It then runs the function normally until a `!` call suspends.

The runner maintains an append-only event history. On replay:

1. A completed event matching the next `!` call supplies its recorded result, so the external operation is not repeated.
2. A scheduled event without a completion keeps the workflow suspended.
3. A new `!` call appends a command event and pauses execution. A worker performs the operation, appends its completion, and schedules another replay.

Code between suspension points must be deterministic. Time, randomness, external reads, and other nondeterministic inputs must go through suspending dependencies so their results enter the history. Runs are pinned to a compatible code version, and suspension sites need stable compiler-generated identities so source edits can be checked during replay.

External operations may run more than once if a worker fails after performing an operation but before recording its completion. The runtime therefore supplies an idempotency key for each scheduled event, and durable providers must either honor it or document weaker delivery guarantees.

There is no `checkpoint` keyword. In v1, the runtime does not serialize the active WebAssembly call stack. It reconstructs local state by replaying from the entry point and reusing recorded suspension results. Capability providers and live resource handles are not stored in workflow history; compatible providers are rebound when execution resumes. Serializable closures are not the primary workflow continuation mechanism, and their separate semantics remain in the backlog.

Interactive notebook-style sessions combine a live kernel with a deterministic execution journal. While the kernel remains alive, closing and reconnecting a client reuses its current namespace without replay. Each successful cell atomically commits a run containing its cell and code identity, parent state, suspension events, state delta, and output.

If the kernel is lost, the runtime restores the latest serializable namespace snapshot and replays subsequent committed cell runs in their actual execution order. Recorded `!` results are reused, giving recovery the same deterministic boundary as durable workflows. Editing and rerunning an earlier cell starts a new history branch from that cell's parent state; runs descended from the previous version become stale. Resume reproduces an existing history, while rerun deliberately creates new computation.

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

struct SpanContext:
    trace_id: string
    span_id: string
    sampled: bool

struct SpanCandidate:
    operation: OperationInfo
    parent: SpanContext?

struct SpanStarted:
    context: SpanContext
    parent: SpanContext?
    operation: OperationInfo
    timestamp: Timestamp

struct SpanEnded:
    context: SpanContext
    outcome: Outcome
    timestamp: Timestamp
    duration: Duration

struct LogRecord:
    level: LogLevel
    message: string
    fields: map[string, ObservationValue]
    span: SpanContext?
    operation: OperationInfo
    timestamp: Timestamp

enum ObservationValue:
    Bool(value: bool)
    Signed(value: i64)
    Unsigned(value: u64)
    Float(value: f64)
    String(value: string)
    List(values: list[ObservationValue])
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

At a declared boundary, returning `Err(error)` maps to `Outcome.Failed` even though `Result` is returned through normal language control flow. Values are not captured by default; only the error type and explicitly supplied safe fields are recorded.

Standard-library logging helpers use the same provider:

```text
fn info(
    message: string,
    fields: map[string, ObservationValue] = {},
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
struct ConsoleObservability:
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
struct OTelObservability:
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
recording := RecordingObservability.new()

$.with(Observability=recording):
    result := process_user!("user-1")

assert_equal(
    recording.log_messages(),
    ["processing user"],
    reason="processing should emit its structured log",
)
```

Fan-out, filtering, redaction, and sampling are provider composition strategies rather than language syntax.

### Replay

Automatic observations receive stable identities derived from execution ID, boundary ID, attempt, and event kind. Deterministic replay does not re-emit observations for already completed history events. New workflow activations use new attempt identities, exporters may deduplicate by observation ID, and replay diagnostics use separate runtime events.

This provider API is an initial draft. Explicit custom-span syntax, metric instruments, privacy/redaction policy, sampling details, and exporter configuration remain open and may be optimized later.
