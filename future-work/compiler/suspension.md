# New Compiler Design: Suspension Lowering

Part of the [compiler design](README.md).

## 14. Suspension Lowering

The spec fixes the protocol: cold, one-shot, poll-based suspensions,
synchronous cancellation, and a waker-driven entry driver
([Compilation Strategy](../../spec/lang/11-requirements-and-suspension.md#compilation-strategy)).
D2 uses state machines with lazily materialized frames, as the research
recommends, which
[`req.lowering.representation`](../../spec/lang/11-requirements-and-suspension.md#r-req.lowering.representation)
allows.

### 14.1 Functions Per Suspending Body

For each instance of a suspending function `f!`:

| Function | Signature | Use |
| --- | --- | --- |
| `f$body` | `(frame: (ref null $F_f), dflt(args)..., dflt(providers)...) -> (dflt(T), (ref null $F_f))` | the body. A null frame starts at state 0 with arguments in locals. A non-null frame resumes from its saved state, and its arguments are default values that it ignores. A null second result means Ready with the first; a frame means Pending, and the first result is `default(T)` |
| `f$cold` | `(args..., providers...) -> (ref $F_f)` | the plain call `f(args)`: allocates a frame holding the arguments and providers in state 0 |
| `f$poll` | `(frame: (ref $Suspend_L), cx) -> (i32, T)` | the vtable's `poll`; casts the frame and calls `f$body` with it |
| `f$cancel` | `(frame: (ref $Suspend_L)) -> ()` | the vtable's `cancel` (§14.6) |

**Defaultable forms (Codex re-review N3).** A value of a data, list or
closure type is a non-null reference, which has no default inhabitant.
So every slot that can be inactive uses the defaultable form `dflt(L)`
of its layout (wasm-layout.md §15.2): each non-null reference becomes
nullable, and `default(L)` is null or zero per Wasm value. Here that is
the Pending result, the arguments of a resume call, and every saved
field of a frame. The state test proves a slot active before it is read,
and the read narrows with `ref.as_non_null`, which cannot fail. On the
fresh path the arguments arrive non-null, and the prologue narrows each
reference argument once into a non-null local.

`$Suspend_L` is one base struct per result layout `L` (§15.2):

```text
$Suspend_L = (sub (struct (field $state (mut i32))      ;; resume point; 0 = not started
                          (field $flags (mut i32))      ;; ACTIVE, STARTED, DONE, CANCELLED bits
                          (field $driver (mut anyref))  ;; the driver that owns it, for competing-driver checks
                          (field $vt (ref $SuspendVT_L))))
$F_f       = (sub final $Suspend_L (struct ... saved locals ... (field $child (mut (ref null $F_g))) ...))
```

### 14.2 The State Machine

Emission builds the state machine from TIR's explicit suspension points;
TIR itself is not changed (§3.10.1):

1. **Liveness.** One backward dataflow over the body's structured
   blocks, iterated to a fixed point on loops (§12.1), gives the live
   set at every suspension point at once. It is not a scan per point,
   which would cost points times instructions. A point's frame fields
   are its live locals and values, plus the locals that the `defer`
   suites of the enclosing `Scope`s read (the point's side record names
   those scopes). The analysis is layout-independent, so it runs once per
   body per run and is memoized.
2. **Blocks containing a suspension point** are flattened into a
   dispatch loop (mine): `loop $dispatch { block_n ... block_1 {
   br_table pc } ... case code ... }`. Each basic block of a flattened
   block becomes one case, and a transfer sets `pc` and branches to
   `$dispatch`.
3. **Blocks without a suspension point** stay structured inside their
   case. Most control flow (a non-suspending loop, an `if`, a match)
   keeps its plain Wasm shape.
4. **Resume points** are the cases that follow each suspension point. The
   prologue reads `frame.state` and sets `pc` to its case, after
   reloading the saved locals.

Code size is linear: one case per basic block of a flattened block, one
save sequence and one reload sequence per suspension point. The prototype
grew about N³ (F-552).

Wasm's structured control flow cannot jump into a nested block, which is
why blocks with a suspension point are flattened. The alternative,
re-entering the nesting with skip flags as Binaryen's Asyncify does,
costs code size on every path. The cost of flattening is a branch through `$dispatch` at
each control transfer inside a suspending loop, a few cycles.

### 14.3 Lazy Frame Materialization

The ready path allocates nothing (C#'s design, as the research says):

```text
;; g!(x) inside f!
call $g$body (ref.null $F_g) x ...       ;; -> (result, child frame)
if child frame is null:                   ;; Ready: continue with the result
    ...
else:                                     ;; Pending
    if f's frame is null: frame = struct.new_default $F_f   ;; every field defaultable
    save the live locals into frame; frame.child = child frame; frame.state = k
    return (default(T), frame)            ;; f is Pending too
```

- A deep chain allocates one frame per level, on the first real wait
  only. Later waits reuse the frames.
- **Dead references are cleared (Codex re-review N-B6).** A reused frame
  would keep a value saved for an earlier wait reachable through a later
  one. So each suspension point's save sequence also stores null into
  every reference field that was live at an earlier point but is dead at
  this one. Liveness (§14.2) already gives both sets, so the clear list
  is known per point at emission. Completion and cancellation null the
  child field and every saved reference, after the `defer` suites ran.
  A capture that a pending `defer` suite reads stays live until that
  suite runs, which liveness already says. Slice 8 tests a large list
  saved across a first wait, then a long second wait, and checks with a
  GC heap measurement that the list was collected.
- **A resumed `f$body` resumes its child directly**: the saved child's
  type is known statically, so `f$body` calls `g$body(child,
  default(args)...)` with no vtable call. The child ignores those
  arguments, since its frame holds what it needs.
- **A stored suspension** (`s!()` on a `mut Suspend[T]`) is polled
  through the vtable: one `call_ref`.
- **A cold call** (`f(x)` without `!`) allocates its frame at once. That
  is the spec's semantics, not overhead
  ([`req.suspend.cold.captures`](../../spec/lang/11-requirements-and-suspension.md#r-req.suspend.cold.captures)).
- **Runtime checks** sit in `f$poll`, not in `f$body`: polling an ACTIVE
  frame panics `suspension-reentrant-poll`, a second driver panics
  `suspension-competing-driver`, and polling a DONE or CANCELLED frame
  panics `suspension-invalid-state`
  ([Runtime Checks](../../spec/lang/11-requirements-and-suspension.md#runtime-checks)).
  A bang call of a direct callee never needs them: its frame is fresh and
  has one driver by construction.

The ready-path cost of `g!(x)` is one direct call and one null test,
which fits the 100 ns `suspension-overhead` budget with room to spare.

### 14.4 Wakers And The Entry Driver

**Wakers (mine).** A waker is a small struct `(task, slot)`: the frame of
an `all!` or the root, and a child index. `wake` sets the slot's bit in
the task's wake mask, propagates the bit up to the root, and sets the
instance's "woken" global. Wakes coalesce in the mask
([`req.waker.coalesced`](../../spec/lang/11-requirements-and-suspension.md#r-req.waker.coalesced)).

Host operations cannot hold GC references across both engines cheaply, so
the instance keeps a **wake table**: a growable array of wakers, indexed
by host handle. The host only ever names handles.

**The entry driver** is a pair of exports:

| Export | Does |
| --- | --- |
| `hd.poll() -> i32` | polls the root (the entry or the running test case); returns `-1` for Pending, or the exit status after `report()` |
| `hd.wake(n: i32)` | reads `n` completed handles from the exchange buffer and invokes their wakers |

```text
host loop:
    status = hd.poll()
    while status == -1:
        if no host operation is pending and no timer is set: deadlock (§14.8)
        wait in the host reactor for at least one completion
        write the completed handles to the exchange buffer; hd.wake(n)
        status = hd.poll()
```

`hd.poll` drains internal wakes before it answers (Codex re-review
N-B3): while the "woken" global is set, it clears it and polls the root
again. So `-1` always means that no wake is queued inside the instance,
and the host loop's deadlock test matches §14.8's definition. Handles
are generation-tagged, and a wake for a stale handle is ignored
(runtime-and-host.md §17.2).

One `hd.wake` call carries every completion of one reactor turn, so a
burst of completions costs one crossing. The driver returns to the host
whenever the root is Pending
([`req.entry.busy-poll`](../../spec/lang/11-requirements-and-suspension.md#r-req.entry.busy-poll)).

### 14.5 `all!` And `race!`

`all!(a, b)` is an intrinsic with one frame type per tuple of child
result types:

- **Fields**: the child frames, one result field per child, a done mask
  and a wake mask.
- **First poll**: poll each child in argument order
  ([`req.schedule.all-unfinished`](../../spec/lang/11-requirements-and-suspension.md#r-req.schedule.all-unfinished)).
  When every child completes on the first poll, `all!` returns the tuple
  from locals and allocates nothing of its own. Only the children's cold
  frames were allocated.
- **Later polls** visit the unfinished children in argument order. A
  child whose wake bit is clear is skipped (mine). Polling it would only
  return Pending with no effect, so skipping is not observable, and it
  makes a wake O(woken children) instead of O(children).
- **Cancellation** cancels the unfinished children in argument order.

`race!(tasks...)` takes a `List[mut Suspend[T]]`:

- It polls the children in list order. The first Ready wins.
- It then cancels every other unfinished child synchronously, in list
  order, before returning
  ([`req.combinator.race-losers`](../../spec/lang/11-requirements-and-suspension.md#r-req.combinator.race-losers)).
- An empty list at run time panics `explicit-panic`.

`all_list!` is std hd over nested `all!` (std's task chapter), so it
needs nothing here.

### 14.6 Cancellation And `defer`

`f$cancel(frame)`:

1. If the frame is ACTIVE on the current poll stack, panic
   `suspension-reentrant-poll` with no state change.
2. If it is DONE or CANCELLED, return
   ([`req.check.cancel-idempotent`](../../spec/lang/11-requirements-and-suspension.md#r-req.check.cancel-idempotent)).
3. Mark it CANCELLED.
4. Cancel the child, if any, through the child's vtable. A child that
   waits on a host operation calls the `hd:rt` abort import for its handle
   ([`req.cancel.external-abort`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.external-abort)).
5. Run the `defer` suites registered at the frame's state, last in, first
   out ([`req.cancel.defer`](../../spec/lang/11-requirements-and-suspension.md#r-req.cancel.defer)).
   `f$cancel` is one `br_table` on the state; each case runs that state's
   suites over the saved locals. Suites cannot suspend, so this is plain
   synchronous code.

A frame that was never polled has no registered suite, so cancelling it
only marks it. Raw abandonment of a started frame runs nothing, as the
spec says.

### 14.7 Hook Points

A `Hook` instruction sits in TIR at each suspension point, resume, frame
creation, completion, cancellation, and host call start and finish. Normal
emission drops it, so it costs nothing in debug or release code.

A trace, replay or simulation build (all Later) sets a flag in the tier
part of `code_key` and emits each hook as a call to an `hd:hook` import
with the site and the frame. Hooks live in the cached TIR, so turning them
on re-emits code entries and never re-checks.

Every nondeterministic input already crosses a host import (time, random,
I/O) or a wake. A simulation host can therefore replay an instance by
feeding recorded host results and wake orders
([`req.determinism.replay`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.replay)).

### 14.8 Deadlock Detection

A deadlock is a root that is Pending while no host operation is pending,
no timer is set and no wake is queued. Nothing can ever wake it.

- **Both tiers** detect it in the host loop for free (§14.4), and fail
  the run instead of hanging. The panic category is open question 3.
- **The debug tier** adds a report. Each frame records the site of its
  current `Await` in its state, so the host walks the frame tree from the
  root and prints each waiting frame's function and source line, as the
  first-release feature list asks.

A `block_on` whose argument waits on a suspension of the outer driver
hangs by the spec's note. That is a deadlock of the same shape inside
`block_on`, and the same check reports it.

### 14.9 `block_on`

`block_on(s)` is std hd over one runtime import:

```text
loop:
    poll s with a waker that sets a local flag
    if Ready: return the value
    if the flag is set: continue
    hd:rt/block()   ;; the host waits for >= 1 completion, writes the handles, returns n
    process the n wakes
```

- **wasmtime:** the import runs the reactor on the same thread until a
  completion arrives. The Wasm stack waits below the host call.
- **Browser:** §17.6.
- **The indirect ban (answer 13).** A global counter counts entered
  forbidden contexts: `defer` suites, `DefaultCall`s whose default body
  makes a call, and module initialization other than the entry's. Facts
  are evaluated at compile time, where reaching `block_on` or `println`
  is the build error `fact-evaluation-failed` (codegen.md §12.3). `block_on` and `println`
  read it and panic when it is not zero. D1 rejects the direct calls at
  check time (§4.13.5). The run-time category is open question 3.
