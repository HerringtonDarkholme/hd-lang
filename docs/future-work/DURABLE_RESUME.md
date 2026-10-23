# Durable execution from a journal

Status: design proposal, 2026-10-23; nothing here is accepted behavior or specification.
This answers [#18](https://github.com/HerringtonDarkholme/hd-lang/issues/18), following the owner's request for a proposal before implementation.
It reviews [Live Execution §6](live-execution.md#6-resume-durable-execution-later), [REPL checkpoints](REPL_CHECKPOINTS.md), and [session rebuilding](../../spec/cli/command-line.md#rebuilding-a-session).

## Today and the missing guarantee

The [native journal](../../crates/hd_run/src/journal.rs) and [JS journal](../../host/journal.mjs) record host results for deterministic REPL replay.
Format 1 has a 16-byte execution identity, per-input source/module identities, indexed CRC-framed rows, and a 64 MiB session cap.
Requests longer than eight bytes retain their length and a 64-bit digest; result bytes remain inline.
The reader returns a valid prefix at either a torn or corrupt row.

These are useful components, but they do not establish crash durability.
The [JS dispatcher](../../host/core.mjs) calls the provider before recording its result; the recorder has no durable intent protocol.
The [native session](../../crates/hd_run_wasmtime/src/session.rs) rebuilds a Store over the same host, retaining that host's resource context.
An old pending-operation number cannot identify a live operation in a fresh process.

The [ABI table](../../crates/hd_host_abi/src/lib.rs) describes codecs, waiting and grant arguments; it does not currently have `opens_handle` metadata.
Today's `Http.send` and `Process.run` return whole values; `Net.connect`, `listen` and `bind_udp` return persistent handles.
Classification must follow actual imports and result contracts, rather than the names of high-level std helpers.

This note pins those observations to `ce769908606e6a2fcbbc5f8a233328d2de9a83e9`.
It makes no new resume throughput, storage or sync-latency claim.

## What must survive

Use the same small program for each option:

```text
println("first")
println("second")
```

Suppose the first write's result is durable, and the second write reaches stdout before the process crashes.
Resume must suppress the first write; the second may repeat because its result never became durable.
This example concerns external output, not reconstructed REPL transcript display.
The same ambiguity applies to a payment or message accepted by another service.

| Situation | Required distinction |
| --- | --- |
| Crash after a completed read | Supply the old bytes, not today's file contents |
| Crash during an external write | Retry can duplicate the write; expose that contract |
| Several operations pending | Reconstruct every unfinished logical operation, not just the last call |
| Changed code, imports or journal | Refuse before any new effect |
| Long execution or large result | Stop admitting work without discarding recovery evidence |
| Socket retained across calls | Refuse initially; a saved integer cannot reopen it |

## Relevant precedents

| System | Mechanism relevant to hd |
| --- | --- |
| [Temporal](https://docs.temporal.io/activity-execution) | Workflow activity outcomes enter history; lost activity tasks can be retried under their retry policy |
| [Restate](https://docs.restate.dev/develop/ts/durable-steps) | `ctx.run` records nondeterministic results in an execution log; failed runs have configurable retries |
| [Azure Durable Task](https://learn.microsoft.com/en-us/azure/durable-task/common/durable-task-code-constraints) | Replayed orchestrators require deterministic APIs, including recorded substitutes for time |
| [Dapr Workflow](https://docs.dapr.io/developing-applications/building-blocks/workflow/workflow-features-concepts/) | Replays completed task results; continue-as-new replaces accumulated history with new execution input |
| [DBOS](https://docs.dbos.dev/architecture) | Stores inputs and step outputs in Postgres; recovery skips checkpointed steps and retries unfinished, preferably idempotent steps |
| [SQLite](https://www.sqlite.org/atomiccommit.html) | Correct persistence depends on ordered writes, flushes and storage guarantees, beyond merely framing a log |

The common boundary is a durable result, not a saved stack frame.
My inference for hd is to reuse capability calls as that boundary, without a new workflow syntax or activity registration API.
The language already routes host observations through those calls; custom providers must meet the same deterministic-replay contract.

## Three mechanisms

| Mechanism | Example after the crash above | What it costs |
| --- | --- | --- |
| **Durable host-call replay** | Suppress `first`; retry `second`, which may print twice overall | CLI persistence, ABI classification and logical-operation recovery; no new hd form |
| Exported runtime checkpoint | Restore guest state, then reconcile the interrupted second write | All replay ambiguities remain, plus a portable heap/code/resource restoration contract |
| Explicit application state only | Application chooses its own restart cursor and duplicate policy | Ordinary std/application code; smallest mechanism, but no transparent `--resume` |

The first two keep the same hd example unchanged.
The third requires application-specific state and idempotency logic; adding a file write beside `println` cannot atomically commit both effects.
It is useful advice for long executions, rather than an implementation of #18's requested command.
None needs new language syntax; the checkpoint option adds the most runtime machinery.

## Recommendation: strict host-call replay

Ship a local CLI mode which records durable operations, replays their completed prefix, and continues only at the validated end.
Keep changed-code migration, portable heap checkpoints and distributed worker coordination outside the first version.
The initial contract covers process crashes on supported local filesystems; power-loss recovery needs separately verified platform flush and directory-publication guarantees.
Use one exclusive writer for an execution, including during recovery; a file lock does not fence a remote service from an already-sent request.

### Identity and authority

The durable header should identify the exact final Wasm bytes with SHA-256, entry selection, initial arguments, format, compiler/runtime ABI and execution profile.
Keep the required executable available; rebuilding is acceptable only when its identity matches exactly.
Reject changed bytes even when a source edit appears harmless, and reject another engine/profile initially.
Neither hot reload nor REPL longest-prefix reload authorizes migration of this durable execution.

Record provider-contract identities and required authority, but require the caller to supply current providers and grants on resume.
Validate both before replay can become live; matching an old manifest never grants new authority.
Nondeterministic input, including initial host observations, must enter through recorded boundaries.
An opaque custom import without a complete replay/resource contract is refused.

### Durable intent, then durable result

For each potentially effecting operation, append and sync an intent before invoking the provider.
Append and sync its outcome, including errors and result bytes, before guest code observes completion.
Persist scheduling choices, cancellation transitions and terminal outcomes before exposing the corresponding transition to the guest.
Coalescing writes is allowed only when it preserves these ordering barriers; there is no one-sync-per-effect cost promise.

Pure observations, such as random bytes, need their result durable before use, even when no intent is needed.
Treat all unclassified provider calls as potentially effecting rather than guessing that a method is a read.
After a journal write or sync failure, stop execution and preserve the prefix; never continue with recording disabled.
The effect may already have happened when persisting its result fails.

| Last durable state | Recovery action |
| --- | --- |
| No intent | This operation was not authorized to start; run it when reached live |
| Intent, no durable outcome | Retry with the same logical operation identity; duplication is possible |
| Durable outcome | Serve that outcome without calling the provider |
| Durable program termination | Return the recorded terminal status without running the program live |

This is at-least-once recovery of unfinished effects, assuming eventual successful recovery, rather than exactly-once external delivery.
Use an application-level idempotency key when the destination supports deduplication; the runner cannot make arbitrary destinations transactional.
A recorded cancellation means the guest abandoned the operation, not that an already-issued external effect was undone.
Recovery must preserve that abandonment and must not restart the abandoned operation.

### Pending operations and time

Assign durable logical operation IDs independently of transient host handles.
During replay, completed operations use recorded results and pending operations use virtual IDs, with recorded poll/wake/cancel ordering.
Re-execution reconstructs and validates request arguments; digest-only rows are not independently executable retry requests.
Keep the reconstructed argument bytes until recovery reaches the validated end.

At that boundary, reissue all unfinished, non-abandoned operations and map their logical IDs to fresh host handles.
An operation may now complete immediately even if its old start returned Pending; buffer that completion until the guest's next valid observation.
Do not perform these retries while still validating the earlier prefix, or turn a replay mismatch into a live call.
There may be several unfinished calls after concurrent task execution, not the single in-flight call described in the older sketch.

Time needs an explicit provider contract at this transition.
Recommend persisting timer deadlines against recorded wall time, then waiting only the remaining duration; overdue timers become ready after replay.
Rebase resumed monotonic readings so they never precede the last recorded reading, then advance with the new host's monotonic clock.
This requires a durable-mode clock adapter and approval; raw process-relative readings and blindly restarting every sleep are insufficient.

### Refuse resource-opening imports initially

Add complete ABI metadata for methods returning persistent resources; reject either durable flag when any reachable import opens such a resource.
Validate this before initial execution and again before resuming, including custom-profile imports.
Internal provider resources contained within one whole-value call are permitted, but that whole operation must tolerate retries.
Transient IDs for an asynchronous call are not persistent socket/file/child handles.

The static restriction can reject an opening import even when a particular execution never reaches it or promptly closes its result.
Handle-valued data hidden inside a custom codec must also be classified; checking only the top-level return type is insufficient.
Later resource restoration needs its own approved contract, not an assumption that retained integers restore an operating-system object.

### File format and growth

Use a distinct durable format version, retaining indexed frames with length and integrity checks.
Add the identity header, intent/outcome and operation-lifecycle records; keep payloads inline initially to avoid a second file-publication protocol.
Use a cryptographic request digest for large arguments and cryptographic artifact identities; CRC detects damage but does not authenticate a journal.
An implementation must validate index order, lengths, state transitions and supported format before replay.

Ignore only a provably incomplete final append; a complete frame with a bad checksum, invalid lifecycle or missing interior data is an error.
Do not reuse today's reader's treatment of all corruption as a harmless tail cut.
Never truncate an existing file merely to make replay succeed; preserve damaged bytes for inspection.
Creating the journal and publishing its identity must complete before the first effect is authorized.

Recommend a configurable **64 MiB admission threshold**, distinct from a hard maximum file size.
Once reached, admit no new operations; already admitted operations may append completion evidence beyond it.
Unknown result lengths and concurrent pending operations prevent a truthful hard cap without reserving maximum result space before every start.
Report the actual size and stop at the next admission boundary; require a larger limit before continuing an exhausted execution.

No automatic rotation deletes active history, and exhaustion is never reported as successful program termination.
Disk exhaustion can still prevent a completed effect's outcome from being stored, leaving the same retry ambiguity as a crash.
Inline results can be large; blob storage and a strict disk quota would require a later atomic-publication and reservation design.

### Checkpoints and public commands

Do not require implicit checkpoints for the first durable mode.
The [#646 proposal](REPL_CHECKPOINTS.md) retains the original provider context and archives history; it does not establish portable crash restoration.
Its measured snapshot primitive excludes persistence, handles and complete checkpoint generation.
Future durable checkpoints must be committed recovery artifacts with a matching code/runtime identity before old history can be discarded.

Applications can bound their history by finishing an execution with explicit durable state and starting another.
That is a pattern, not a new built-in continue-as-new API or an atomic handoff guarantee.
If the handoff must occur exactly once, the application needs a transactional store or idempotent coordinator.

Recommend exposing `hd run --journal PATH` and `hd run --resume PATH` together, after crash-injection tests establish this contract.
`--journal` creates a new execution and refuses an existing destination; `--resume` validates and appends to its existing execution.
Defer separate public `--record` and `--replay` flags: diagnostic recording without intent durability would otherwise look misleadingly interchangeable with resume.
Internal replay remains useful for testing; a future public read-only replay should refuse incomplete history instead of continuing live.

## Approval and implementation proof

Approval is needed for strict identity, at-least-once retries, logical operation recovery, durable clock behavior, resource refusal, storage limits and paired public flags.
The recommendation favors existing host boundaries over a new activity API, accepting conservative eligibility and replay-from-start cost.
The next-best simplification is explicit application state when transparent recovery does not justify that runtime contract.
No code or spec change follows from this note alone.

An implementation order should first prove fresh-process recovery for a completed read, an interrupted write, several pending operations, abandonment and an overdue timer.
Inject a crash before/after each append, sync, provider invocation and result delivery; verify output counts and exact recovery outcomes.
Also cover wrong identity, concurrent writers, torn tails, interior corruption, sync failure, exhausted admission budget and forbidden resource imports on Node and Wasmtime.
Use a fault-injecting provider to observe duplicate attempts; successful ordinary runs alone cannot establish the guarantee.

Future CLI-tier rules belong in [command-line.md](../../spec/cli/command-line.md); ABI/runtime representation belongs in host design records.
Any new diagnostic code belongs in the [language diagnostic inventory](../../spec/README.md#diagnostics); exact codes remain for the owner's spec order.
No std API or new syntax is required by this recommendation, and no conformance exception is added here.

## Example validation

The two-line example uses existing syntax only; parsing and checking it validate the example, not the proposed durability behavior.
The implementation proof above remains unperformed.
