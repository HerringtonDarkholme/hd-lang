# Wake-Driven Host Entries

Status: Deferred proposal, not accepted language behavior or a completed compiler repair. Owner requested recording it here and fixing it later.

## Problem

The native suspending entry export currently drives its frame synchronously until Ready.
Repeatedly polling Pending can block the JavaScript event loop that must complete the asynchronous host operation.
The relevant requirements are [entry pending and busy polling](../spec/lang/11-requirements-and-suspension.md), and the compiler audit's A06 finding.

## Proposed Embedding Interface

An instantiated program would expose a JavaScript host entry interface, separate from raw Wasm exports.
Ordinary entries would return directly; suspending entries would return a Promise.
The wrapper would coordinate native start, poll, cancel, and result operations.

After the first poll returns Pending, the driver must yield and wait for a retained host waker.
Wakes must coalesce without concurrent or reentrant polling.
A stale waker must not affect a later execution.
Failure cleanup must run once and preserve the original failure if cleanup also fails.
The instance's providers and execution frame must survive each Pending interval.

## Implementation And Verification Plan

1. Establish the exact native entry protocol and registered JavaScript interface.
2. Implement a wake-driven wrapper with execution-specific wakers and an explicit cancellation policy.
3. Remove the synchronous public suspension export; retain documented low-level protocol operations.
4. Migrate affected suspension tests to await the host interface, preserving their expected language results.
5. Cover delayed wakes, synchronous wakes, duplicate wakes, stale wakes, competing drivers, cancellation, provider retention, and poisoning.
6. Run the complete compiler suite before integrating this repair independently.

The test migration needs permission beyond the earlier `src/`-only implementation scope.
The owner deferred this proposal before granting that permission.
Experimental work in the larger repair worktree is not evidence that this interface is accepted or ready to ship.
