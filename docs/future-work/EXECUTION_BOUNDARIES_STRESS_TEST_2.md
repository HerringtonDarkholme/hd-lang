# Execution Domains Stress Test, Round 2

Status: design review, 2026-11-02; changes no decision, design record,
spec text, or compiler code. It tests
[EXECUTION_BOUNDARIES.md](EXECUTION_BOUNDARIES.md) at 22577cd58, the
rewrite on a five-part core, against the 18 cases of
[round 1](EXECUTION_BOUNDARIES_STRESS_TEST.md), and maps each round-1
problem to its status.

- [Surface Being Tested](#surface-being-tested)
- [Method](#method)
- [Summary](#summary)
- [Cases](#cases)
- [Round-1 Problems](#round-1-problems)
- [New Problems, Ranked](#new-problems-ranked)
- [Parse Log](#parse-log)

## Surface Being Tested

| Part | As the record states it |
| --- | --- |
| Domains | run-time values from providers; kinds are std types (`Scope`, `Background`, `Workers`, `Jobs`, servers, `Gpus`) |
| Bounds | `Send`, `Sync`, `Wire`, structural; a closure is `Wire` when its captures are, `Send` when its captures are and it writes none |
| Handles | `Handle` marker; home recorded at run time; sent only to its home (`WrongHome`); `Wire` as home and id |
| Remote | R1 bang methods, `Wire` parameters and results, not generic, no row; R2 `E < From[CrossError]`; R3 `serve!(impl)` and `remote::[T](domain)` |
| CrossError | `Unreachable`, `Unknown`, `OtherBuild`, `WrongHome` |
| Builds | B1 to B5: a walk per target, dispatcher, stub, codecs, build ID |

Assumption X1: a served object other than an `Action` (case 9) is a std
`Served[T]` handle, which the record does not state.

## Method

The cases are round 1's, so verdicts compare directly. Each case is
judged *works*, *friction* (works with a workaround or a visible
difference) or *breaks* (cannot be written, or behaves differently).
Blocks are parsed, not type-checked. Where a case's code is already in
the record, the case cites that block instead of repeating it.

## Summary

| # | Case (library) | Round 1, V2 | Round 2 | Part at fault |
| --- | --- | --- | --- | --- |
| 1 | Worker thumbnail (Comlink, transferables) | friction | friction | moves into separate memory copy the bytes (N1); capture by move (N6) |
| 2 | Form action (Next.js "use server") | friction | friction | handles must be unforgeable (N2) |
| 3 | Typed service (tRPC, gRPC) | friction | friction | per-request context (P15) |
| 4 | Scoped count and sort (thread::scope, rayon) | breaks | works | none |
| 5 | Off the UI thread (Kotlin, Swift) | works | works | none |
| 6 | Delayed mail (Sidekiq, Temporal) | breaks | friction | a deploy fails pending jobs (`OtherBuild`) |
| 7 | GPU kernels (wgpu, CUDA) | breaks | friction | device implementation choice (N4); device subset and geometry (P17) |
| 8 | Data-local count (Ray, Spark) | breaks | friction | reaching the resource at home (N5) |
| 9 | Long-lived process (Erlang, Akka) | breaks | friction | served objects are stated only as `Action` (N7) |
| 10 | Test swaps the server | works | works | none |
| 11 | Error propagation | friction | works | none |
| 12 | Crossings in `join!` | works | works | none |
| 13 | Cancel mid-crossing | friction | works | none |
| 14 | Remote call inside a remote call | friction | works | none |
| 15 | Remote with a closure or a handle | breaks | friction | `Wire` closures contradict R1 (N3) |
| 16 | Generic remote function | breaks | friction | a generic method is barred; a generic trait works |
| 17 | Streaming results | breaks | friction | a stream is a handle pulled per item |
| 18 | Background result updates UI | friction | works | none |

Totals: 8 works, 10 friction, 0 breaks (round 1, V2: 3, 7, 8). The rewrite
closes every *breaks*. The frictions cluster on handles (N2, N5, N7) and on
moves into separate memory (N1, N6). One finding, N3, is an internal
contradiction, and N2 is a security gap.

## Cases

### 1. Worker Thumbnail (Comlink, Transferables)

Comlink posts a call to a worker, and a transferable hands an
`ArrayBuffer` over without copying. The record's section 4 block 1 is the
translation: `Photo` is `impl !Copy`, so it moves, and the result returns
it. **Friction.** Two Wasm instances have separate linear memories, so
the bytes are copied once in bulk; only JavaScript can hand a buffer over
without a copy (N1). A closure also captures by cell
([`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)); nothing says a crossing's closure takes a `!Copy`
capture by move and ends the source binding (N6).

```text
# hypothetical syntax; Photo is impl !Copy with Wire fields
let mut workers = $.use(Workers)
let (back, small) = workers.run!(fn() -> (Photo, Photo): thumbnail(photo, 256))?
show(small)
keep(back)
```

### 2. Form Action (Next.js "use server")

Next.js serializes a server action as an id the client posts back. Section
4 block 7 is the translation. **Friction.** The captured todo id stays on
the server, as wanted. But an `Action` held by a browser is a handle that
an untrusted peer holds and returns. Nothing says its id is unguessable or
bound to the session that received it, so a page could forge another
user's action (N2). Next.js encrypts closed-over values for the same
reason.

### 3. Typed Service (tRPC, gRPC)

The record's section 5 program is the translation. **Friction.** A
`Remote` method has no row, and its implementation reaches resources
through fields. The caller's identity is per request, so it cannot be a
field. gRPC passes it in call metadata, tRPC in a context object (P15).

### 4. Scoped Count And Sort (std::thread::scope, rayon)

**Works.** Section 4 block 5 borrows `lines` in a `Scope`. A spawn that
writes a captured counter is rejected, which closes round 1's race (P2).

```text
# hypothetical syntax; lines: &List[string]
let total = +0
counts := scope(fn(s: &mut Scope) -> List[i32]:
    tasks := [for i in 0..4 => s.spawn(fn() -> i32: count_quarter(lines, i))]
    s.spawn(fn() -> i32: total += 1)         # error: unsatisfied-trait-bound, a closure that writes total is not Send
    [for t in tasks => t.join()]
)
```

### 5. Off The UI Thread (Kotlin, Swift)

**Works.** Section 4 block 8: `background.run!` returns a plain `T`, the
handler resumes on the UI thread, and a closure that uses `$ Ui` does not
fit `Background`'s row.

### 6. Delayed Mail (Sidekiq, Temporal)

**Friction.** Section 4 block 3 works within one build. Every deploy fails
the jobs queued before it with `OtherBuild`. That is the stated design: it
is loud and clear, but a team that deploys daily cannot queue a job for a
week.

### 7. GPU Kernels (wgpu, CUDA)

**Friction.** Section 4 block 4 reads well. Two details are missing. The
device target needs an entry that selects which `Remote` implementation
it compiles, since a device has no `main` (N4). Launch geometry and the
device-code subset are unspecified (P17).

### 8. Data-Local Count (Ray, Spark)

**Friction.** `workers.at(rows)` sends the code to `rows`'s home. At home,
the code needs the resource behind the handle, a table, but the record
gives no way to reach it. A handle's resource is reachable only by
"crossing to its home", which the code already did (N5).

### 9. Long-Lived Process (Erlang, Akka)

**Friction.** An account process is a stateful object on a worker, called
by message. It fits if a domain can serve an object and return a handle
to it. The record states that only for `Action` (N7). Supervision and
mailbox order are library work.

```text
# hypothetical syntax; an account process on a worker, as an Erlang process
pub trait Account < Remote:
    fn deposit!(&mut self, cents: i64) -> Result[i64, AccountError]
    fn balance!(&mut self) -> Result[i64, AccountError]

struct Ledger:
    cents: i64

impl Account for Ledger:
    fn deposit!(&mut self, cents: i64) -> Result[i64, AccountError]:
        self.cents += cents
        .Ok(self.cents)
    fn balance!(&mut self) -> Result[i64, AccountError]:
        .Ok(self.cents)

pub fn open_account!() -> Result[i64, AccountError] $ Workers:
    let mut workers = $.use(Workers)
    let mut worker = workers.pick()
    account := worker.spawn!(Ledger { cents: 0 })?       # account: Served[Account], home: that worker
    let mut acct = remote::[Account](account)
    acct.deposit!(500)
```

### 10. A Test Swaps The Server

**Works.** A stub, a real implementation and a fake are all providers of
`Users`; `$.with(Users=PgUsers {...})` even runs the real one in process.

### 11. Error Propagation

**Works.** `E < From[CrossError]` with one `@from` variant lets `?` carry
a lost contact as the method's own error. Round 1's dead-case friction
(P11) remains only as a variant the server never builds.

### 12. Crossings In `join!`

**Works.** Two bang calls on two domain values join as any two calls do.

### 13. Cancel Mid-Crossing

**Works.** Abandoning sends a cancel; the outcome is `Unknown`, and a
moved value never returns through an error.

### 14. A Remote Call Inside A Remote Call

**Works.** A server implementation holds another service's stub as a
field and maps that service's `CrossError` into its own error.

### 15. Remote With A Closure Or A Handle

**Friction.** A handle parameter works and crosses to its home. A closure
parameter should be rejected, since another build must not run code a
caller picks. But R1 asks only for `Wire` parameters, and the bound table
makes a closure `Wire` when its captures are. As written, R1 admits the
closure (N3).

```text
# hypothetical syntax; a Remote method that takes a closure is rejected by R1
pub trait Sorter < Remote:
    fn sort_by!(&mut self, rows: List[Row], key: fn(Row) -> i64) -> Result[List[Row], SortError]   # error: R1, a closure parameter

pub trait Thumbs < Remote:
    fn shrink!(&mut self, tex: Texture, side: u32) -> Result[Texture, GpuError]   # valid: a handle crosses to its home
```

### 16. Generic Remote Function

**Friction.** A generic method is barred (R1). A generic trait served at
one type works, as Rust's object-safe traits allow.

```text
# hypothetical syntax; a generic method is rejected, a generic trait served at one type is not
pub trait Store < Remote:
    fn get![T < Wire](&mut self, key: string) -> Result[T, StoreError]   # error: R1, a generic method

pub trait TypedStore[T < Wire] < Remote:
    fn get!(&mut self, key: string) -> Result[T, StoreError]             # valid: serve!(UserStore {}) fixes T = User
```

### 17. Streaming Results

**Friction.** A stream is a handle whose home is the server, pulled one
item per crossing. It works, at one round trip per item; server push
needs a batching cursor in the library.

```text
# hypothetical syntax; streaming as a handle pulled item by item
pub trait Feed < Remote:
    fn open!(&mut self, topic: string) -> Result[Stream[Event], FeedError]   # a handle whose home is the server

pub fn watch!(topic: string) -> Result[void, FeedError] $ Feed + Console:
    let mut feed = $.use(Feed)
    events := feed.open!(topic)?
    let mut cursor = remote::[Next[Event]](events)
    while true:
        match cursor.next!()?:
            .Some(event) => println("$event")
            .None => break
    .Ok(())
```

### 18. A Background Result Updates UI

**Works.** The handler gets the value back on the UI thread and updates
state there. A closure that captured the UI state's box would not be
`Send`, which is the intended rejection.

## Round-1 Problems

| ID | Round-1 problem | Status in round 2 |
| --- | --- | --- |
| P1 | two builds talk in every deploy | closed by design: `OtherBuild` names it; daily deploys still fail week-long jobs (case 6) |
| P2 | scoped threads take boxes and written captures | closed: a box is not `Send`, a closure that writes a capture is not `Send` |
| P3 | domain instances must be static types | closed: domains and homes are run-time values |
| P4 | an immutable top-level box escapes the top-level rule | closed: reached top-level bindings must be immutable and `Wire` |
| P5 | two shapes for one crossing | closed: one shape |
| P6 | an unmarked caller has no domain | closed with V1 |
| P7 | a remote call looks local | open, by design: the trait, the row and the error type tell |
| P8 | handles barred from interfaces | closed: handles are `Wire` |
| P9 | a crossing returns one value | closed: a tuple |
| P10 | unknown has several spellings | closed: four cases, a timeout after sending is `Unknown` |
| P11 | crossing failure in result types | closed: `From[CrossError]` |
| P12 | no strategy for threads, kernels, ids | partly: section 5 Running and section 6 Cost list them; the device backend is open |
| P13 | a local test server's semantics | closed: a real implementation is a provider |
| P14 | jobs: handles across restarts | open: handle lifetime when contact is lost (N8) |
| P15 | per-request capabilities on a server | open |
| P16 | an endpoint cannot use rows | closed by design: implementations use fields; it moves the per-request part to P15 |
| P17 | kernels: geometry and device subset | open |
| P18 | `serve!` with two `Remote` traits | open: the record does not say what `serve!` serves when a type implements two (N9) |
| P19 | internal contradictions | N3 remains |

## New Problems, Ranked

| Rank | ID | Problem | Severity | Cases |
| ---: | --- | --- | --- | --- |
| 1 | N2 | a handle that an untrusted peer holds can be forged | high: security | 2, 9, 17 |
| 2 | N3 | `Wire` closures contradict R1 | high: the record contradicts itself | 15 |
| 3 | N5 | code at a handle's home has no way to reach the resource | medium | 8, 9 |
| 4 | N7 | served objects are stated only as `Action` | medium | 2, 9, 17 |
| 5 | N6 | a crossing's closure takes `!Copy` captures by cell, not by move | medium | 1 |
| 6 | N1 | a move into separate Wasm memory is a bulk copy, not a hand-over | medium: the record overclaims | 1 |
| 7 | N8 | handle lifetime when its holder loses contact | medium | 2, 9, 17 |
| 8 | N9 | `serve!` with two `Remote` traits | low | 3 |
| 9 | N4 | how a device target picks its implementation | low | 7 |

### N2. A Handle An Untrusted Peer Holds Can Be Forged

**Effect.** Cases 2, 9 and 17 give a browser a handle whose home is the
server. A handle crosses as its home and id. A page that guesses or edits
an id calls another user's action, account or stream. The `WrongHome`
check proves the home, not the right to use it.

**Candidates.**
1. A handle sent to another build carries an unguessable token, bound to
   the session it was sent to; the home refuses any other with
   `WrongHome`.
2. Only handles of the caller's build may leave a trust boundary; a server
   wraps each in a signed token, as Next.js encrypts action closures.

### N3. `Wire` Closures Contradict R1

**Effect.** The bound table makes a closure `Wire` when its captures are,
so `Workers.run!` can take one. R1 asks a `Remote` method only for `Wire`
parameters, so it admits a closure, which lets a caller pick code another
build runs (case 15).

**Candidates.**
1. R1 says its parameters and results are `Wire` and not closures.
2. Closures are never `Wire`; same-build domains bound the code by its
   captures instead, through a separate marker.

### N5. Code At A Handle's Home Cannot Reach The Resource

**Effect.** `workers.at(rows).run!(...)` runs where `rows` lives, but the
record gives the code there no way to turn the handle into the table it
names (cases 8 and 9).

**Candidates.**
1. At its home, a handle's own methods reach the resource directly; away
   from home they are crossings. The handle type is a `Remote` trait
   served by its home.
2. A std `home_get(handle)` at the home, refused elsewhere.

### N7. Served Objects Are Stated Only As `Action`

**Effect.** An Erlang process, a stream cursor and an action are all
"an object a domain serves, reached by handle". The record states only
`Action` (cases 2, 9, 17).

**Candidates.**
1. A std `Served[T]` handle for any `Remote` trait `T`; `Action` and
   `Stream` are instances of it.

### N6. Capture By Move At A Crossing

**Effect.** A closure captures by cell. Sending one with a `!Copy`
capture must move the value and end the source binding, as Swift's
`sending` does, but the record does not say so (case 1).

**Candidates.**
1. A closure passed to a crossing takes each `!Copy` capture by move; a
   later use of the source binding is `use-of-moved-value`.

### N1. A Move Into Separate Wasm Memory Copies The Bytes

**Effect.** The record says example 1's buffer "is handed over". Two Wasm
instances have separate linear memories, so a move copies the bytes once,
in bulk. Only the source binding ends.

**Candidates.**
1. Say so: a move into separate memory ends the source binding and copies
   the bytes once, never encoding them field by field.

### N8. Handle Lifetime When Contact Is Lost

**Effect.** A home keeps a resource alive until its last holder drops the
handle. A browser that closes without dropping it leaks the server's
action, account or stream forever.

**Candidates.**
1. A handle held across a contact-losing boundary is a lease, renewed by
   use and dropped by its home after a timeout; a later use is `WrongHome`.

### N9. `serve!` With Two Remote Traits

**Effect.** A type that implements `Users` and `Orders` passed to
`serve!` is ambiguous in the record (case 3).

**Candidates.**
1. `serve!(impl)` serves every `Remote` trait its argument implements.

### N4. How A Device Target Picks Its Implementation

**Effect.** A device has no `main`, so B1's walk has no entry, and the
record does not say which `Remote` implementation the device build
compiles (case 7).

**Candidates.**
1. The device target's entry is a module with `pub fn device_main() ->
   void: serve!(DeviceFilters {})`, walked as any entry.

## Parse Log

The record's blocks are parsed in its own Parse Log. This report's six
blocks were parsed on 2026-11-02 with `scripts/hd-conformance.sh parse`;
parsing checks syntax only.

| Block | Case | Result |
| --- | --- | --- |
| 1 | 1, worker thumbnail | parses; `# hypothetical syntax` |
| 2 | 4, scoped count | parses; `# hypothetical syntax` |
| 3 | 9, account process | parses; `# hypothetical syntax` |
| 4 | 15, closure and handle parameters | parses after one fix of mine: a function type is `fn(Row) -> i64`, not `fn(row: Row) -> i64` |
| 5 | 16, generic method and generic trait | parses; `# hypothetical syntax` |
| 6 | 17, streaming | parses; `# hypothetical syntax` |
