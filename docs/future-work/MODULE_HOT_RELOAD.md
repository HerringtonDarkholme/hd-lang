# Module Hot Reload

Design proposal, 2026-10-23; nothing here is accepted behavior or implemented
by this note. It addresses [#182](https://github.com/HerringtonDarkholme/hd-lang/issues/182)
under the [owner's design-note order](https://github.com/HerringtonDarkholme/hd-lang/issues/58#issuecomment-6369999711).
The owner approves the direction before specification edits or implementation.

## Contents

- [Purpose and current behavior](#purpose-and-current-behavior)
- [Experience elsewhere](#experience-elsewhere)
- [Choices and recommendation](#choices-and-recommendation)
- [What can change](#what-can-change)
- [Publishing and retaining code](#publishing-and-retaining-code)
- [REPL and checkpoint interaction](#repl-and-checkpoint-interaction)
- [Delivery and validation](#delivery-and-validation)
- [Evidence and example checks](#evidence-and-example-checks)

## Purpose and current behavior

A server or frontend developer should change a function body while keeping
live application state. A waiting task must resume safely, and an
incompatible edit must require a restart. A failed build must leave the
running program alone.

[The reserved hot-reload table](../compiler/engines-and-test-runner.md#185-the-reserved-hot-reload-table)
is not built. This does not mean the compiler has no function table.
The linear backend already uses an internal `funcref` table for closures,
trait dispatch, and runtime glue; ordinary calls still use direct calls.
Its emitted table has a fixed maximum and no host-facing reload API.

| Current piece | Limit relevant to reload |
| --- | --- |
| `module.rs` function symbols, calls, and table slots | Existing indices describe one linked module; they are not stable update identities. |
| Suspension bodies and generated frame helpers | Resumption, cancellation, and release depend on the body version that created the frame. |
| Closure code/environment pairs and static trait vtables | Replacing a code slot can mismatch its existing environment or receiver representation. |
| Linear heap, runtime globals, and static data | A replacement module cannot independently own another allocator over the same live heap. |
| Native and Node REPL sessions | The current hosts rebuild and replay kept inputs between inputs; they do not retain one live module. |

The [REPL specification](../../spec/cli/command-line.md#redefinition) forbids
redeclaring an earlier input's name. Its
[`:reload`](../../spec/cli/command-line.md#r-cli.repl.command.reload) rebuilds the session,
keeping the longest matching replay prefix without repeating host effects.
Hot reload must not silently replace either rule.

## Experience elsewhere

| System | Documented behavior | Lesson for hd |
| --- | --- | --- |
| [Erlang/OTP](https://www.erlang.org/doc/system/code_loading.html) | Old and current module code coexist; qualified calls select current code. Purging lingering old code can terminate its processes. | Separate choosing new code from reclaiming old code; do not adopt forced purging implicitly. |
| [Java instrumentation](https://docs.oracle.com/en/java/javase/25/docs/api/java.instrument/java/lang/instrument/Instrumentation.html#redefineClasses(java.lang.instrument.ClassDefinition...)) | Active frames continue their original bytecode; subsequent invocations use redefined methods. Redefinition restricts structural changes. | Keep active frame code and reject incompatible contracts. |
| [Dart/Flutter](https://docs.flutter.dev/tools/hot-reload) | Hot reload preserves application state, but does not rerun global or static initializers. Some type changes require restarting. | New code does not make old state equivalent to a fresh run. |
| [.NET](https://learn.microsoft.com/en-us/visualstudio/debugger/supported-code-changes-csharp?view=visualstudio) | Supported edits depend on the compiler/runtime; unsupported edits require restarting. Active statements have restrictions. | Report the precise rejected edit rather than promising every body edit works. |
| [Vite](https://vite.dev/guide/api-hmr) | Modules accept updates explicitly; invalidation propagates to importers. Module data can survive through a dedicated object. | Define update boundaries and state ownership explicitly. |
| [Clojure](https://clojure.org/reference/vars) | Functions stored in Vars can be redefined through indirection. | Indirection changes future lookup; [captured function objects](https://clojure.org/guides/faq) can retain the old function. |

These systems offer different guarantees. Retaining old code and preserving
state are separate mechanisms, and neither proves that old and new
algorithms agree about that state.

## Choices and recommendation

All three choices use ordinary hd source. For example, the first version is:

```text
use std.time.{Clock, sleep, ms}

fn adjusted(x: i32) -> i32: x + 1

pub fn main!() -> void $ Console + Clock:
    first := adjusted(+10)
    sleep!(1ms)
    println("$first ${adjusted(+10)}")
```

The developer changes only `adjusted`:

```text
use std.time.{Clock, sleep, ms}

fn adjusted(x: i32) -> i32: x + 2

pub fn main!() -> void $ Console + Clock:
    first := adjusted(+10)
    sleep!(1ms)
    println("$first ${adjusted(+10)}")
```

The comparison assumes publication happens while `main!` waits. The
one-millisecond delay illustrates suspension; it is not a reliable reload
test synchronizer. An implementation test needs a controlled host gate.

| Criterion | Restart on every edit | Drain before replacing | Retain active bodies |
| --- | --- | --- | --- |
| Mechanism | Build a fresh instance; explicitly restart. | Wait until affected activations finish, then replace compatible bodies. | Keep each active body version; future named calls use the published compatible version. |
| Example | A fresh run prints `12 12`; prior state is lost. | The waiting old run prints `11 11`; publication waits for it to finish. | The old `main!` resumes and prints `11 12`. |
| Long-running server loop | Restarts it. | Can block publication indefinitely. | Its next named handler call can use new code. |
| Suspended task | Existing run must end before restart. | Blocks the update. | Resumes its original body and frame. |
| Live heap | New heap. | Retained when contracts are compatible. | Retained, with versioned code lifetimes. |
| Reading and writing | Simple restart notice. | Simple code, potentially surprising indefinite waits. | Explicit mixed-version calls; existing local values remain unchanged. |
| Cost and soundness | Smallest tooling design; no code migration. | Needs safe dispatch and a reliable quiescence test. | Needs compatible dispatch, old-code retention, and runtime ownership. |
| Rule cost | CLI lifecycle only. | Runtime/tooling contract plus CLI policy. | Runtime/tooling contract plus CLI policy. |
| Evolution | Useful fallback, no state-preserving reload. | Suitable for bounded jobs. | Can support servers and frontends without migrating active frames. |

Restarting is the radical simplification and the next-best first delivery.
It provides a watcher without claiming state-preserving hot reload. None
of these choices requires a new hd keyword or a new standard-library trait.

**Recommendation.** Develop the third option as an opt-in development mode,
with compatible body replacement and retained active bodies. Keep restart
as an explicit fallback when compatibility cannot be established.
The proposed spelling `hd run --hot` needs CLI approval; it is not a current flag.

An activation is one entered function body, including its suspended lifetime.
It keeps its original code, locals, and cleanup logic. A later named call
from that body selects the current compatible callee, even if the caller
started before publication.

This deliberately permits old and new algorithms to share state. It does
not promise one coherent source revision for a whole request. Pinning the
entire transitive `main!` invocation would leave a server loop on old code
forever; automatic request boundaries would require another mechanism.

## What can change

Compare checked contracts and lowered representations, not source spelling
or a matching Wasm parameter list alone. Begin conservatively:

| Edit | Proposed action |
| --- | --- |
| Function or method body, with unchanged checked contract and shared representations | Publish after the complete candidate passes checks. |
| Locals or suspension points inside that body | Give new activations new frames; keep old frames with old helpers. |
| Parameter/result types, bang status, generic parameters/bounds, associated types, any requirement-row change, or any default-argument contract change | Require restart. |
| Struct/enum layout, trait method set, implementation selection, or reachable nominal type identities | Require restart. |
| Named declaration added, removed, or moved | Require restart in the first implementation. |
| Module initializer or top-level binding initializer | Require restart; do not rerun it against the live heap. |
| Runtime ABI, profile, imports, grants, package configuration, or dependency identity | Require restart. |
| Parse/type/link failure or unsupported lowering | Keep the running generation; report the failure. |

Compiler-generated closures, frames, and helpers are part of their code
version. New internal layouts must never reinterpret an existing object.
An implementation may reject a body edit it cannot isolate; the diagnostic
must identify the unsupported dependency.

Existing function values need distinct treatment. Recommend that a named
function value uses a stable entry adapter, selecting the latest compatible
body when invoked. A closure keeps its original code/environment pairing;
its body may still make later named calls.

Trait method adapters may update only when receiver and dispatch contracts match.

Release, tracing, cancellation, and abandonment helpers follow the objects
or frames they describe. They must not become latest-version lookups merely
because their Wasm signatures match. This includes vtables and glue reached
only when the last reference disappears.

## Publishing and retaining code

The watcher observes package sources and the dependency closure the running
program actually reaches. It builds one consistent source snapshot,
including affected generic instances and generated adapters. Publication
is a single transaction for that candidate, not one update per saved file.

The host publishes between Wasm entries, with no Wasm stack active. A
pending hd task can remain in its explicit suspension frame. A synchronous
computation that never returns to the driver delays publication; the watcher
does not rewrite its stack or interrupt it to install code.
A host wait inside a suspended Wasm stack is not this safe point; that
stack must return before publication.

Build, compatibility checking, loading, and dispatch preparation precede
publication. They must not rerun user initializers or make application host
calls. A partial failure leaves the previous dispatch state intact.

A candidate identifier records its source snapshot and runtime ABI. Named
slots need stable item and specialization identities, independent of dense
Wasm function numbers. The publication changes only the approved entry adapters.
Hot builds must preserve that indirection across inlining, specialization, and
constant folding; a stale inlined callee would violate the proposed policy.

The existing internal table is insufficient on its own. Code generations
need one owner for live memories, mutable runtime globals, allocator,
collector, and storage. Replacement code imports that state through a
checked private runtime ABI rather than instantiating a second independent runtime.
These code modules form one hd program instance; separate program instances
retain the [specified disjoint heaps](../../spec/lang/10-modules.md#instances-and-threads).

Static data and descriptors also need generation-owned ranges. Existing
closures, pending frames, host callbacks, and release/trace glue retain the
code and metadata they can still call. Old source maps remain available
so diagnostics name the code that actually ran.

RC counts on hd values do not automatically count those code dependencies.
The implementation must prove their lifetime tracking before reclaiming
code or recycling slots. If retained versions reach a configured budget,
refuse another publication and offer restart; never kill an old task implicitly.

A patch does not cancel a pending host operation, replay its effect, change
its handle identity, or rebind its providers. Cancellation still invokes
the frame's original cleanup path and honors the existing cleanup bound.
A runtime panic retains the existing poisoning behavior; reload cannot repair it.

A body-only edit can still invalidate an application's semantic invariant.
For example, changing units from cents to dollars preserves an `i64` layout
but misinterprets old values. The developer must restart for such changes;
structural compatibility is a memory-safety check, not a state migration proof.

## REPL and checkpoint interaction

Keep the initial hot mode separate from the REPL. Entering a second `fn f`
at the prompt remains an error. `:reload` continues rebuilding and replaying
the longest matching prefix under the existing CLI rules.

Do not turn `:reload` into a table update, and do not automatically replay
a running server's accumulated effects after an incompatible edit. A normal
restart starts a new program; replay remains the session mechanism with its
existing divergence checks.

[Checkpoint compaction, #646](https://github.com/HerringtonDarkholme/hd-lang/issues/646),
is being designed separately and is not accepted by this note. A checkpoint
must identify its code and runtime representations. Reusing its pointers
under changed layouts is invalid, even when public signatures match.

An unchanged-code checkpoint could eventually accelerate a rebuild.
Applying new code to a checkpoint needs a separately approved compatibility
and historical-prefix policy. Until then, retain existing `:reload` replay
behavior; checkpointing must not silently discard the history that policy needs.

Hot-update events and retained generations would also need an explicit
recording format before cross-version replay or durable execution can work.
This proposal makes neither promise. It does not broaden the REPL's
[deferred hot-reload discussion](live-execution.md#101-what-the-framing-gets-for-free-later)
into an accepted redefinition feature.

## Delivery and validation

The owner must approve the call-version policy, restart boundary, trigger,
version-retention policy, and separation from REPL reload before code.
Later tasks should specify runtime/tooling contracts and CLI rules in their
own tiers. This note changes no specification rule or ID.

First demonstrate a pure named call on both official hosts with one
replacement and unchanged heap ownership. Then exercise a controlled
suspension, old cleanup, and eventual code reclamation. Only after those
proofs should a filesystem watcher or framework integration advertise hot reload.

| Required check | Evidence |
| --- | --- |
| Body edit while a task waits | Old locals and frame resume safely; a later named call reaches the new body. |
| Closure and trait values held before the edit | Environment pairing and receiver layout remain valid. |
| Old value released after several updates | Correct release/trace glue remains callable until no retained edge exists. |
| Cancel/abandon after update | Original cleanup and host-operation identities remain valid. |
| Contract/layout/initializer change | Restart required; old state remains intact until explicit restart. |
| Candidate fails or publication allocation fails | Previous generation continues without partial dispatch changes or user effects. |
| Repeated updates with a long-lived old closure | Retention budget refuses more updates without purging live code. |
| REPL redeclaration and `:reload` | Existing diagnostics, longest-prefix replay, and host-once behavior stay unchanged. |

Measure build, validation, publication, call overhead, and retained code/data
separately. This note reports no reload latency or speedup: no implementation
exists to measure. Keep optimized ordinary builds free of the development dispatch machinery.

## Evidence and example checks

Source inspection uses `d4b85c0be845fb426bc1476d65693d35f9dae3b0`:

| Source | Fact inspected |
| --- | --- |
| [Compiler host design §18.5](../compiler/engines-and-test-runner.md#185-the-reserved-hot-reload-table) | Reload table is reserved, not implemented. |
| [Linear module linker](../../crates/hd_wasm_linear/src/module.rs) | Direct calls, internal indirect-call table, generated frame helper identities, static data, and runtime globals. |
| [Closure emission](../../crates/hd_wasm_linear/src/emit/closure.rs) | Code slot and environment are paired. |
| [Native session](../../crates/hd_run_wasmtime/src/session.rs) and [Node session](../../host/core.mjs) | Session state is reconstructed through recorded inputs. |
| [REPL implementation](../../crates/hd_cli/src/repl.rs) | `:reload` rereads package scope and rebuilds, refusing a full journal. |
| [REPL specification](../../spec/cli/command-line.md#rebuilding-a-session) | No repeated host effects and longest matching prefix. |

Both complete hd examples parse cleanly and check with zero errors and warnings.
The frozen parser and compiler are from `bc3ffb05a3` plus the #526
cancellation-pruning change; they are not a fresh build of the inspected revision.
These checks validate ordinary source, not the proposed update mechanism.

The parse log contains two parser successes and two checker successes.
Exact commands, binary provenance, source hashes, and outputs accompany
this note's external validation evidence. No reload experiment or Rust build
was run for this document.
