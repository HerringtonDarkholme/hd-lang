# Durable execution from a journal

Status: design exploration, 2026-10-23; direction chosen by the owner the same day (see [Direction](#direction-host-call-replay-with-activities)); nothing is in the specification yet.
This answers [#18](https://github.com/HerringtonDarkholme/hd-lang/issues/18)
and its [exploration review](https://github.com/HerringtonDarkholme/hd-lang/issues/18#issuecomment-6372711365).
It revisits [Live Execution §6](live-execution.md#6-resume-durable-execution-later),
[deterministic execution](../../spec/lang/11-requirements-and-suspension.md#determinism),
and [session rebuilding](../../spec/cli/command-line.md#rebuilding-a-session).
The earlier proposal is evidence and one option, not an approved implementation plan.

## Contents

- [The domain](#the-domain)
- [What must survive](#what-must-survive)
- [Today and the missing guarantee](#today-and-the-missing-guarantee)
- [Relevant precedents](#relevant-precedents)
- [Three mechanisms](#three-mechanisms)
- [Explicit application state](#explicit-application-state-the-smallest-option)
- [Explicit durable steps](#explicit-durable-steps)
- [Automatic host-call replay](#automatic-host-call-replay)
- [Runtime checkpoints](#runtime-checkpoints)
- [Comparison on the fixed use cases](#comparison-on-the-fixed-use-cases)
- [Direction](#direction-host-call-replay-with-activities)
- [Approval and implementation proof](#approval-and-implementation-proof)
- [Sources and example validation](#example-validation)

## The domain

Durable execution means recovering a computation after its process disappears.
Its variables, stack, and live operating-system resources disappear with that process.
The outside world does not roll back: a server may already have accepted a payment, or stdout may already contain a line.
The recovery mechanism must connect durable local evidence with that outside history.

A journal records observations and decisions so the same program can reconstruct its state.
A checkpoint records a state from which execution can restart, possibly with a smaller journal afterward.
Neither word implies that arbitrary external effects become transactional.
Saving a machine's memory and saving evidence about a remote effect solve different parts of the problem.

| Term | Meaning in this exploration |
| --- | --- |
| Logical operation | One intended action, with a stable execution-relative identity across retry attempts. |
| Attempt | One invocation of that action; several attempts may reach the destination. |
| Intent | Durable permission to start an operation, recorded before an effect can escape. |
| Outcome | Durable success/error/result evidence, committed before the guest observes completion. |
| Replay | Re-execute code while serving recorded outcomes instead of repeating completed external work. |
| Recovery frontier | The point after all earlier history has been validated, where unfinished or new operations may become live. |
| Idempotency | Repeating the same logical request has the same business effect, usually through destination-side deduplication. |
| Checkpoint | Captured execution or application state; it needs its own code, resource, and publication contract. |
| Durability barrier | A storage operation whose acknowledged guarantee survives the stated failure model. A buffered append alone is insufficient. |

### A worked crash, before choosing a mechanism

Consider the same tiny program throughout:

```text
println("first")
println("second")
```

The first line's outcome is safely recorded.
The second line reaches stdout, but the process crashes before its outcome is safely recorded.
Recovery can prove that `first` completed; it cannot tell whether the unfinished `second` intent reached stdout.
A second attempt can duplicate that line, while suppressing the attempt could lose it in an otherwise identical pre-write crash.

| Point | Local durable evidence | Outside world | What recovery can know |
| --- | --- | --- | --- |
| First write completed | Intent and outcome for `first` | `first` exists | Suppress another first write. |
| Second intent committed | Intent only for `second` | Perhaps nothing yet | Second write may need an attempt. |
| Second write accepted, outcome not committed | The same intent only | `second` already exists | This is indistinguishable from the previous row using only the local journal. |
| Second outcome committed | Intent and outcome for both | Both exist | Suppress both completed writes. |

This is the external-effect gap.
Writing “done” before sending can lose the effect; writing it afterward can duplicate it after a crash.
A local transaction cannot include an arbitrary remote server or stdout.
Exactly-once business behavior needs a participating destination, a shared transaction, or an idempotency key with a sufficient retention contract.

The following ordinary hd model demonstrates only that ambiguity:

```text
struct Destination:
    accepted: usize

fn send(destination: &mut Destination) -> void:
    destination.accepted += 1

fn recover(completed: bool, destination: &mut Destination) -> void:
    if !completed:
        send(destination)

pub fn main() -> void:
    uncertain := &mut Destination { accepted: 0 }
    send(uncertain)
    recover(false, uncertain)
    if uncertain.accepted != 2:
        panic("unfinished effect should expose duplicate risk")
    completed := &mut Destination { accepted: 0 }
    send(completed)
    recover(true, completed)
    if completed.accepted != 1:
        panic("completed effect should be suppressed")
```

It asserts two accepted sends for an unfinished operation and one for an already completed operation.
The counters are ordinary memory, not a disk journal or a recovery implementation.
The model makes the duplicate policy visible without claiming a filesystem guarantee.

### Why straightforward answers fail

Restarting the program repeats old writes and can read new file contents instead of the original bytes.
Persisting only the last call loses other unfinished operations when several tasks are pending.
Recording a socket number preserves an integer, not the socket's remote peer or kernel state.
Serializing a compiled Wasm module preserves code, not its live heap or an interrupted host operation.

An empty requirement row is not a serialization guarantee for captured state.
A `defer` suite cannot run after the operating system has killed the process.
A cancelled request may already have reached its destination; cancellation is not external rollback.
These distinctions remain necessary under every option below.

## What must survive

The users are authors of long batch jobs, operators restarting failed runs, and authors deciding whether their external systems support safe retries.
Use the following five cases for every option.
The same visible result and failure points apply even where an option requires different program structure.

| Case | Start, crash, and desired completion |
| --- | --- |
| **Writes** | Run the two-line program; first outcome is durable, second acceptance is uncertain. Finish without repeating known completed work, and expose duplicate risk. |
| **Read then send** | Read an invoice, then send a payment using its business ID. The file changes during downtime; finish using the original invoice and handle uncertain payment acceptance. |
| **Concurrent wait** | Two requests and a timer are pending; one result is durable and another request is durably abandoned. Recover each logical operation and its timing/cancellation decision. |
| **Resource lifetime** | Open a TCP stream, then crash between two uses. Never treat a saved numeric handle as a restored connection. |
| **Operational recovery** | Restart after a code change, damaged history, exhausted storage budget, or concurrent resume attempt. Refuse unsafe recovery before new effects, preserving inspectable evidence. |

## Today and the missing guarantee

The [native journal](../../crates/hd_run/src/journal.rs) and [JS journal](../../host/journal.mjs) record host results for deterministic REPL replay.
Format 1 has a 16-byte execution identity, per-input source/module identities, indexed CRC-framed rows, and a 64 MiB session cap.
Requests longer than eight bytes retain their length and a 64-bit digest; result bytes remain inline.
The current reader returns a valid prefix at either a torn or corrupt row.

These are useful components, but they do not establish crash durability.
The [JS dispatcher](../../host/core.mjs) invokes the provider before recording its result; its recorder has no durable intent barrier.
The [native session](../../crates/hd_run_wasmtime/src/session.rs) rebuilds a Store over the same host, retaining that host's resource context.
A pending-operation number cannot identify a live operation in a fresh process.

The [ABI table](../../crates/hd_host_abi/src/lib.rs) describes codecs, waiting, and grant arguments; it has no `opens_handle` metadata today.
`Http.send` and `Process.run` return whole values; `Net.connect`, `listen`, and `bind_udp` return persistent handles.
Eligibility must follow actual imports and complete result contracts, not the name of a high-level std helper.
A transient asynchronous operation ID and a persistent stream handle require different recovery treatment.

The earlier source observations were pinned to `ce769908606e6a2fcbbc5f8a233328d2de9a83e9`; this exploration rechecked those facts at `ad3edc0f2`.
The previous note contains no measured resume throughput, storage throughput, or sync latency; none is invented here.
The [#646 evidence](https://github.com/HerringtonDarkholme/hd-lang/issues/646#issuecomment-6370301464) remains available, although that separate checkpoint design is deferred.
Its same-code memory-copy experiment is evidence about a primitive, not a dependency on an approved checkpoint solution.

At its pinned `d4b85c0be` build, a 67,104,885-byte journal replay took retained/discarded medians of 2110.40/2131.32 ms on Wasmtime.
Those native trials recreated the Store over the same host; Node's fresh-host medians were 996.51/932.70 ms, and actual Chrome's were 760.50/753.00 ms.
All medians use three shared-host trials; these are historical observations, not current-build performance claims.
The exact-code snapshot copied 134,414,336 bytes in either workload and omitted handles, persistence, and complete checkpoint generation.

The [existing determinism rules](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends)
identify the compiled program, runtime profile, entry arguments, ordered host results, and wake/cancellation deliveries as inputs.
The [REPL rebuilding rule](../../spec/cli/command-line.md#r-cli.repl.rebuild) permits replay without repeating host calls.
It neither specifies durable storage nor authorizes a changed durable program's longest-prefix recovery.
No public durable-run flags are implemented by this exploration.

The initial CLI comparison concerns Node and Wasmtime; browser REPL persistence is a separate contract, not a proof of durable execution.

## Relevant precedents

### Tool-by-tool survey

The survey separates the programmer's interface from the recovery boundary.
The systems below solve related problems, but their external-service guarantees depend on the contracts around each boundary.

| System | Design and interface | Coverage | Limits relevant to hd |
| --- | --- | --- | --- |
| [Temporal](https://docs.temporal.io/activity-execution) | Workflow code schedules registered activities; the service records activity lifecycle and outcomes in event history. | Durable task scheduling, retries, timers, and workflow reconstruction. | Lost activity attempts retry according to timeout/retry policy; arbitrary external effects still need idempotency. |
| [Restate](https://docs.restate.dev/develop/ts/durable-steps) | `ctx.run` wraps a nondeterministic operation and stores its serialized result in an execution log. | Completed steps replay from stored results; retries and deterministic time/random helpers are available. | Nondeterministic operations outside the boundary break replay; an unfinished step can execute again. Nested context operations inside `run` are restricted. |
| [Azure Durable Task](https://learn.microsoft.com/en-us/azure/durable-task/common/durable-task-code-constraints) | Deterministic orchestrators call activities and durable timers through a context. | Event-sourced orchestration with recorded task results and resumable waits. | Direct I/O, ordinary clocks, and arbitrary async work do not belong in replayed orchestration code. |
| [Dapr Workflow](https://docs.dapr.io/developing-applications/building-blocks/workflow/workflow-features-concepts/) | Workflow functions schedule activities, timers, and event waits; the engine maintains workflow history. | Recovery, activity retries, workflow-instance management, and durable waits. | Determinism and code-update restrictions remain; persistence is provided by configured infrastructure, not arbitrary local variables. |
| [DBOS](https://docs.dbos.dev/architecture) | Annotated workflows and steps checkpoint inputs/results in Postgres, with recovery coordinated through that store. | Application-integrated workflows, step recovery, queues, and shared database state. | Step outputs must be serializable; external steps need their own retry/idempotency policy. A completed local record cannot atomically include an unrelated destination. |
| [SQLite](https://www.sqlite.org/atomiccommit.html) | Transactions use ordered writes, locks, journal data, and flush barriers to publish one committed database state. | Local atomic data commits under documented filesystem/hardware assumptions. | A local database transaction does not commit stdout or an independent payment service. Broken locking or flush guarantees undermine recovery. |
| [Wasmtime](https://docs.wasmtime.dev/api/wasmtime/struct.Module.html#method.serialize) | `Module::serialize` saves compiled code for later loading and instantiation. | Avoids recompiling a compatible compiled module. | A module is code; an instance's memory, globals, execution frames, and host resources are separate state. This API is not a durable workflow engine. |

Temporal's [activity guidance](https://docs.temporal.io/activity-definition#idempotency) explicitly distinguishes repeatable activity attempts from business idempotency.
Its [workflow replay model](https://docs.temporal.io/workflows) reconstructs local state from event history rather than saving an ordinary stack.
Those facts support replay as a mechanism, not a claim that every call happens exactly once.

Restate's durable-step boundary requires nondeterminism to be wrapped, including database and HTTP calls.
Its recorded results and retry rules make the unresolved-attempt gap visible.
The useful hd comparison is automatic host boundaries versus programmer-selected steps, rather than borrowing its API spelling.

Azure's restrictions explain why a replayed orchestration must obtain time and I/O through recorded operations.
Dapr similarly exposes workflow-aware activities and timers, instead of persisting arbitrary async work automatically.
Both demonstrate that cancellation, timers, and recorded scheduling belong in the recovery contract.

DBOS's [step documentation](https://docs.dbos.dev/python/tutorials/step-tutorial) requires serializable outputs and defines how nested steps behave.
That boundary can group several host calls into one retried application step.
Its database integration does not mean a generic wrapper can transact with every external service.

SQLite demonstrates the difference between framing a log and committing evidence durably.
A file append returning successfully can still leave bytes in an operating-system cache.
Its protocol is specific to storage it controls; it cannot remove the external-effect gap for a remote destination.

The takeaways are three separate choices: where to record, what to retry, and what storage acknowledgement means.
Replay frameworks generally make the first two explicit, while relying on a durable storage service for the third.
A checkpoint can reduce replay cost without changing the requirement for effect reconciliation.

## Three mechanisms

The earlier note compared replay, checkpoints, and explicit application state.
This exploration retains that heading and adds a fourth mechanism: programmer-selected durable steps.
The difference is the unit whose outcome suppresses re-execution, not a different spelling of `--resume`.
The following code models are ordinary hd; no proposed durable API is implemented by them.

### Explicit application state: the smallest option

The application stores a typed cursor or business record and starts a fresh ordinary run from that state.
The runner does not reconstruct the program automatically.
This is the radical simplification: no new runtime mode, journal format, or portable heap contract.
It gives up transparent whole-program resume and shifts progress/idempotency decisions to the application.

```text
struct Cursor:
    next: usize

fn continue_from(cursor: Cursor) -> Cursor $ Console:
    if cursor.next == 0:
        println("first")
    if cursor.next < 2:
        println("second")
    Cursor { next: 2 }

pub fn main() -> void $ Console:
    restored := Cursor { next: 1 }
    finished := continue_from(restored)
    if finished.next != 2:
        panic("cursor did not advance")
```

This example starts from a supplied cursor after `first` and prints only `second`.
It does not persist that cursor; a real application must load it from its chosen durable store.
Saving the cursor after output still leaves the same acceptance gap.
A transaction helps only when the business effect and cursor participate in the same transactional system.

For **Writes**, the operator loads cursor 1 and runs the remaining write; uncertain acceptance can still duplicate it.
For **Read then send**, the application must save the original invoice and business key, not reread a changed file accidentally.
For **Concurrent wait**, it must represent every pending operation and deadline itself.
There is no automatic scheduler reconstruction.

For **Resource lifetime**, a new run reconnects or refuses according to application policy; old integers are never sufficient.
For **Operational recovery**, the application owns state-schema versions, locking, storage failure, and code migration.
Ordinary hd values, enums, `Result`, and existing serialization can represent its domain state.
Captures, trait values, and RC object graphs are not implicitly serialized by this pattern.

This option adds no language or CLI rule.
A reusable helper would be a stdlib-tier API only after a concrete storage protocol is chosen; no such API is proposed here.
Its implementation cost is application-specific progress records and a transactional or idempotent destination.
It has the smallest compiler/runtime cost, but the largest repeated burden on every application author.

### Explicit durable steps

The author groups nondeterministic work into named steps whose serializable outcomes are committed to a durable store.
On restart, deterministic orchestration runs again; completed steps return their saved outcomes without invoking their bodies.
Unfinished steps retry as whole units, possibly repeating several host calls within a step.
This resembles Restate or DBOS boundaries rather than recording every host import automatically.

```text
fn step_model(completed: bool, body: fn() -> void $ Console) -> void $ Console:
    if !completed:
        body()

pub fn main() -> void $ Console:
    step_model(true, fn(): println("first"))
    step_model(false, fn(): println("second"))
```

`step_model` deliberately receives an already-decided completion bit and has no persistence implementation.
Its output is `second`, showing the skip-versus-execute rule.
A real API additionally needs stable step identity, recorded inputs, typed result codecs, an atomic durable store, and request/version validation.
The model does not supply those missing pieces.

For **Writes**, two steps can suppress the first and retry the second.
One step containing both writes retries both if its overall outcome was not committed; choosing the boundary changes observable duplication.
For **Read then send**, separate read/send steps preserve invoice bytes, while one large unfinished step may reread them.
A stable business key still belongs to the payment request.

For **Concurrent wait**, a durable step scheduler must recover independent IDs, results, deadlines, and cancellation, or initially reject that shape.
For **Resource lifetime**, a resource confined to one retried step can reopen; a resource escaping between steps needs an explicit restore contract or rejection.
For **Operational recovery**, names alone cannot validate a changed body or state schema.
Retain exact program identity initially, or design explicit step versioning before permitting code changes.

A library can express typed closures, capability rows, and serializable results using existing language mechanisms.
It still needs a storage provider with a real durable-commit contract; ordinary `FsWrite` success alone does not establish that contract.
A future step API belongs in the stdlib tier when built over an explicitly registered provider, with no new syntax or intrinsic.
Any default compiler-known host boundary would require its own approved ABI and language-tier inventory treatment.

The soundness obligation is complete confinement of nondeterminism to recorded steps or recorded orchestration primitives.
An empty callback row is insufficient evidence about serializability or destination-side idempotency.
Step storage cannot save arbitrary closures, references, or trait values as opaque bytes and call them portable results.
This option trades smaller journal volume for explicit programmer discipline and coarser retry units.

### Automatic host-call replay

The runtime wraps each supported host import, recording intents and outcomes with durable barriers.
After a crash, it re-executes the exact program while supplying completed outcomes and recorded scheduling inputs.
Only after validating history does it resume unfinished or new live operations.
The application keeps the original source shape:

```text
pub fn main() -> void $ Console:
    println("first")
    println("second")
```

The ordinary run prints both lines.
A future validated recovery would suppress the completed first host write and retry the uncertain second one.
That conditional behavior is not implemented by this program or the current REPL recorder.
It requires the protocol discussed below.

For **Writes**, the host boundary provides automatic fine-grained suppression, with duplicate risk for an unfinished write.
For **Read then send**, the recorded read returns the original invoice; the send uses the same logical identity and business key.
For **Concurrent wait**, the recorder must preserve the full pending set and delivery order, rather than only the last call.
For **Resource lifetime**, the conservative initial import restriction rejects the stream-using program before effects.

For **Operational recovery**, exact code/profile identities, a validated journal, and one writer are prerequisites before continuing live.
This approach reuses hd's [host boundary and determinism model](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.replay).
It adds CLI and host/runtime protocol behavior, not language syntax or a std workflow API.
The implementation cost includes both dispatchers, durable storage, logical-operation recovery, and cross-engine crash tests.

### Identity and authority

For an automatic mode, the header would bind exact Wasm bytes, entry selection, initial arguments, runtime/profile identity, and format version.
Strict identity is the simplest code policy; compatible deployment changes require a separate versioning/migration mechanism.
Grant and provider contracts must also be validated before replay or live continuation, using current supplied authority.
A journal never recreates a grant that the new caller did not supply.

A custom registered provider participates only with a complete deterministic-replay and resource contract.
Ordinary hd provider logic is part of the compiled program; external provider behavior needs a separately pinned contract identity.
[Import needs](../../spec/cli/command-line.md#r-cli.cap.total.needs) give an exact boundary list, not a complete resource or idempotency classification.
Missing metadata means ineligible, rather than permission to guess.

### Durable intent, then durable result

A potentially effecting operation needs its intent durable before provider dispatch, and its outcome durable before guest completion.
Random bytes and other pure observations still need their outcomes durable before the program depends on them.
Wake, cancellation, abandonment, and terminal decisions also need ordered durable evidence.
There is no established “one sync per effect” cost: intent and outcome are two distinct barriers, although safe batching may amortize physical flushes.

| Last durable evidence | Available policy choices |
| --- | --- |
| No intent | The operation was not permitted to start; run it when reached live. |
| Intent without outcome | Retry the same logical operation, stop for reconciliation, or require a destination idempotency contract. The journal alone cannot choose exactly once. |
| Outcome | Serve it without invoking the provider again. |
| Abandonment | Preserve the abandoned state; do not restart the operation merely because it lacks an ordinary result. |
| Program termination | Return the durable terminal result without starting new work. |

Under the retry choice, unfinished effects have at-least-once attempts given successful eventual recovery.
That is not a bound of one duplicate: repeated crashes can cause repeated attempts.
Errors are outcomes too, and an accepted destination action may outlive a locally failed attempt.
A process-local journal lock does not fence a remote request that was already sent.

### Pending operations and time

Durable logical IDs must be separate from ephemeral host handles.
Replay can reconstruct completed results and virtual pending operations without making live calls while earlier history remains unvalidated.
At the frontier, every unfinished non-abandoned operation needs either a fresh attempt or an explicit reconciliation state.
An immediately ready new attempt must still respect the recorded program's observation order.

Large request digests validate reconstructed arguments; they are not executable retry payloads by themselves.
The same program must reconstruct and retain those arguments before crossing the frontier.
Recorded wake and cancellation ordering remains part of [deterministic input](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.depends).
Cancellation does not undo an external write or guarantee a remote attempt stopped.

[Clock readings](../../spec/std/time.md#r-std-time.clock.monotonic) need a durable-mode policy.
Absolute wall-clock deadlines count downtime, but depend on wall-clock correction behavior.
Restarting the remaining duration avoids a wall-clock assumption but can extend a timeout by the downtime.
A new process's raw monotonic origin cannot simply be appended after the old process's readings.

One candidate adapter preserves recorded reads, then rebases monotonic time above the last observed value and treats expired wall deadlines as ready.
That adapter changes durable-mode behavior and needs approval and clock-jump tests.
It is not a consequence of saving `Clock.sleep`'s request alone.
The same issue exists in an explicit step scheduler and a runtime checkpoint system.

### Refuse resource-opening imports initially

An initial automatic mode could reject any import that opens a resource surviving one host call.
This matches the conservative option in [Live Execution §6.4](live-execution.md#64-what-cannot-be-resumed), but the ABI classification is not implemented.
It can reject an import even when a particular execution never reaches it or quickly closes its result.
That is a visible eligibility limit, not an approximation of portable resource restoration.

Whole-value HTTP or process calls can be retried as units when their provider contracts allow it.
That does not make a persistent socket, listener, file, or child handle restorable from its integer value.
Handle-containing custom codecs also need classification; inspecting only a top-level result type is insufficient.
Allowing portable resources later requires explicit reopen/restore semantics, not simply dropping this guard.

### File format and growth

Today's format is useful for replay, but a durable variant needs versioned identity, operation lifecycle, barrier semantics, and validated record transitions.
CRC can detect accidental damage; it does not authenticate untrusted journal content.
A complete bad-checksum frame or invalid interior transition must not be treated as harmless truncation.
Only a provably incomplete final append can be ignored under a specified torn-tail policy.

Inline outcomes are the smallest storage mechanism, with no separate blob-publication transaction.
Large results and many admitted pending operations can still grow the file beyond an admission threshold.
A **64 MiB threshold** is a candidate inherited from the REPL, not a measured durable-storage optimum or a hard file bound.
After admission closes, pending completions still need durable evidence; disabling recording while effects continue is unsafe.

A strict disk quota instead requires worst-case reservation, bounded results, or an external blob store with its own atomic publication protocol.
Disk failure after an external effect can leave an unfinished intent, just like a crash.
Storage exhaustion is a recoverable failure state, not successful program termination.
Journal creation, exclusive-writer locking, protected file access, and publication durability belong to the storage contract.

### Checkpoints and public commands

Automatic replay can start without checkpoints, paying replay-from-start cost.
Long runs may finish with explicit application state and start another execution, but that handoff needs a transactional or idempotent coordinator if duplication matters.
The deferred [#646 note](REPL_CHECKPOINTS.md) concerns REPL compaction and original-provider context; it does not establish fresh-process crash recovery.
No accepted checkpoint dependency blocks exploring a journal-only first version.

The proposed public surface could be paired `hd run --journal PATH` and `hd run --resume PATH` flags.
A new journal would refuse an existing destination; resume would validate and append to an existing execution.
Separate diagnostic `--record`/`--replay` flags could ship first, but result recording without durability must not look interchangeable with resume.
Keeping those diagnostic flags private initially is another choice, not an existing CLI rule.

### Runtime checkpoints

A runtime checkpoint saves a guest continuation at a supported boundary, then restores it with matching code and a small operation journal.
It can avoid replaying an ever-growing prefix when complete state capture is available.
It still needs durable intent/outcome reconciliation for effects crossing the checkpoint boundary.
A heap snapshot alone cannot identify whether the second write reached stdout.

```text
pub fn main() -> void $ Console:
    println("first")
    # A proposed runtime checkpoint would resume before the second write.
    println("second")
```

The comment marks a proposed capture boundary; it calls no checkpoint API.
Ordinary execution still prints both lines, so parsing or running this file does not prove checkpoint recovery.
A working runtime would need a serializable continuation at that boundary or an explicit compiler-generated state-machine point.
Arbitrary native/Wasm call stacks are not captured by today's memory-copy experiment.

For **Writes**, a checkpoint after the first durable outcome can skip prefix computation, while the uncertain second write still needs retry or reconciliation.
For **Read then send**, it must capture the invoice, references, and code identities, while retaining payment-operation evidence.
For **Concurrent wait**, it needs the guest task graph plus a journal of logical operations, deadlines, and cancellation.
Restoring pending numeric handles is insufficient.

For **Resource lifetime**, it either rejects live resources or requires matching external resource restore contracts.
For **Operational recovery**, code/layout changes, partial checkpoint publication, and a missing checkpoint are additional failure cases.
Old history can be deleted only after a complete recovery artifact is durably committed.
Otherwise a failed checkpoint can destroy the only route back to valid state.

Capturing RC headers without the allocator, collector, glue/table identities, roots, and ownership counts is unsound.
Captured closures and trait values also refer to code and dispatch metadata that must match after restore.
A checkpoint that keeps the original host's resources is a same-process continuation, not this issue's fresh-process durability guarantee.
This option adds compiler/runtime representation and CLI/storage contracts, with no need for new user syntax.

Its complete cost is unknown: the historical primitive copied 134,414,336 bytes and excluded resource handling, serialization, and durable publication.
No transparent checkpoint implementation exists in this proposal.
This is a larger independent mechanism, not a small optimization to make the current journal durable.

## Comparison on the fixed use cases

| Criterion | Explicit application state | Explicit durable steps | Automatic host-call replay | Runtime checkpoints |
| --- | --- | --- | --- | --- |
| Writes | Load cursor; retry/reconcile uncertain line. Application commits progress. | Skip completed first step; retry second. A coarse combined step can repeat both. | Skip first outcome; retry/reconcile second intent after full validation. | Restore after first boundary; reconcile second through a separate operation log. |
| Read then send | Store original invoice/key explicitly; resume from business state. | Cache read separately; retry send with same key. Grouped unfinished steps may reread. | Replay original bytes; reissue unfinished send with reconstructed arguments. | Restore invoice state; reconcile unfinished send, retaining code/layout identity. |
| Concurrent wait | Application encodes pending set, deadlines, and abandonment. | Needs a durable scheduler or initial rejection of concurrent steps. | Restore all logical IDs and ordered observations; do not retry abandoned work. | Restore task graph plus logical-operation/timer journal. |
| Resource lifetime | Reconnect through application logic or refuse. | Reopen inside a retried step; reject un-restorable cross-step handles. | Conservative opening-import refusal before effects. | Reject resources or implement explicit fresh-host restore contracts. |
| Operational recovery | Application validates schema, locking, budget, and business migrations. | Validate step/program identity and store; fail before a new body executes. | Strict identity, validated records, exclusive writer, admission control before live frontier. | All journal safeguards plus atomic checkpoint publication and code/layout validation. |
| Rules added or retired | None; optional application/storage libraries. | Proposed stdlib API and provider contract; no syntax. | Proposed CLI durable mode and host/runtime protocol; no syntax or std workflow API. | Proposed CLI/runtime checkpoint contract; no user syntax. |
| Soundness boundary | Cursor and business effects must share a transaction or deduplication policy. | All nondeterminism confined; typed results truly serializable. | All nondeterministic imports/deliveries covered; completed outcomes never reissued. | Guest continuation, heap, code, runtime, and resource state agree. |
| Agent writing | Explicit domain state each application must get right. | Step boundaries, keys, codecs, and retries are explicit. | Existing programs qualify or receive a precise import/identity refusal. | Existing source, but eligibility and migration limits need clear diagnostics. |
| Human reading | Progress is visible in source; generic durability is absent. | Retry unit is visible, including accidental grouping. | Source is unchanged; CLI mode must prominently state unfinished-effect semantics. | Source hides substantial captured runtime state and restore prerequisites. |
| Implementation cost | Smallest compiler cost; repeated application/storage work. | Step/result store, library contracts, scheduler, codecs, crash tests. | Two host integrations, durable writer, full operation lifecycle, metadata, crash tests. | Guest capture/restore, stable layouts/RC state, storage publication, plus effect reconciliation. |
| Evolution | Domain-specific migrations and explicit rotation. | Versioned step schemas and optional new scheduling primitives. | Checkpoints or version markers can be added only under separate contracts. | Capture can reduce replay cost, but upgrades require explicit state migration. |

The differences are mechanism costs and guarantees, not a promise that any option removes destination-side retry hazards.
The smallest option drops transparent resume; the checkpoint option adds the most representation work.
A library-first step API is cheaper in language mechanisms than a new syntax form, but does not make durable storage free.
No option needs a new grammar form or compiler intrinsic by default.

## Direction: host-call replay with activities

The direction is automatic host-call replay, as #18 designed it from the start.
The runtime records every host call, and recovery re-runs the unchanged program while serving the recorded outcomes, then continues live from the recovery frontier.
Golem takes the same approach for Wasm components, because a Wasm guest reaches the world only through host calls.

An intent whose outcome is unknown is retried.
Every unfinished effect therefore runs at least once, as in Temporal and Restate, and each request carries its stable logical-operation identity so that a destination can remove duplicates.
Stopping for an operator, or admitting only destinations that remove duplicates themselves, would need a person at every crash or would reject ordinary programs such as one that calls `println`.

Long-lived handles live only inside activities, as in Temporal.
An activity is a block that is retried as a whole: it may open files, sockets or connections, and only its result, plain data that can be saved, crosses back into the recorded program.
A crash inside an activity re-runs it from its start, and the activity reopens what it needs, seeking from saved progress such as an offset when that matters.
A program that opens a long-lived handle outside an activity is refused before any effect, by the classification in [Refuse resource-opening imports initially](#refuse-resource-opening-imports-initially).
The activity's exact API is still to be designed.

Nothing ships publicly yet: neither `--journal`/`--resume` nor a public `--record`/`--replay`.
The first work is record and replay, because it has no live phase.
A replay serves recorded outcomes, performs no live effect, and stops at the first divergence, so its log needs no durability barrier.
It is useful on its own for reproducing a run exactly, and it is the base that journal and resume later extend with durable barriers, a single writer, identity checks and the live frontier.

This gives up changed-code resume, persistent handles outside activities, bounded replay cost, and universal exactly-once external effects.
The first durable proof targets process-crash recovery on supported local storage, with a documented acknowledgement contract.
Power-loss guarantees need separately verified platform flush and directory-publication behaviour; ordinary append tests do not establish them.
There is no measured durable-sync latency or storage-cost claim here.

## Approval and implementation proof

No compiler, std, host, CLI, or specification behavior is changed by this exploration.
A subsequent scoped issue would name files and initial eligibility precisely before implementation.
CLI behavior would belong in the [CLI tier](../../spec/cli/command-line.md);
new diagnostic codes would enter the [language diagnostic inventory](../../spec/README.md#diagnostics).

Existing determinism, references, ownership, trait, suspension, and cancellation rules remain unchanged.
Only the chosen storage/recovery boundary adds new obligations.
Public std APIs would need stdlib rules and fixtures; compiler-known ABI entries would need their language-tier treatment.
No rule ID is retired by this document.

A useful implementation proof must crash between append, durable acknowledgement, dispatch, destination acceptance, outcome commit, and guest delivery.
It must test a changed file read, uncertain write, multiple pending requests, durable abandonment, and an overdue timer in fresh processes.
Wrong identity, corrupt interior records, a torn tail, denied grants, concurrent writers, storage failures, and exhausted admission also need negative cases.
Successful ordinary execution or a same-host replay is insufficient proof.

## Example validation

The six distinct hd blocks are complete current-syntax workloads or explicitly labeled in-memory models.
The question repeats the same two-write workload.
They were parsed, checked, individually formatted, and run on Node and wasmtime using a frozen current-source compiler.
None invokes a durable flag or asserts that crash recovery is implemented.

All six checks report zero errors/warnings; twelve ordinary engine runs have the exact documented output or successful model assertions.
The fresh job base is `ad3edc0f242d5c87b923d6b2a5f7ae53f81b39b7`.
All compiler/std/host sources are unchanged from frozen build `a93482cb9`;
CLI/parser hashes are recorded with the raw results, not inferred from a nearby binary name.

A separate exhaustive model explores eleven single-operation crash/persistence states and 121 pairs of independent operations.
It shows the missing-outcome duplicate/loss trade-off and why recovery must account for every unfinished logical ID.
This is a protocol-state model, not physical crash injection, filesystem testing, or an end-to-end runtime proof.
Original source facts and the separately pinned checkpoint measurements remain cited above.

Primary sources are linked at each survey claim; the [future-work index](README.md) already links this record.
Documentation gates and exact committed hashes are reported with the branch delivery.
