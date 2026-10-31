# Execution Boundaries: Moving Code And Data

Status: proposal, 2026-10-31; domain: language (execution domains and
crossings); not accepted. Nothing here is in the specification. It revises
the [PR #1139](https://github.com/HerringtonDarkholme/hd-lang/pull/1139)
brainstorm after the owner's review: examples first, then the model, then
type checking, then syntax. The parts the owner approved are marked
**approved**; everything else is a proposal.

- [1. Context And Goal](#1-context-and-goal)
- [2. Six Programs That Cross](#2-six-programs-that-cross)
- [3. The Model](#3-the-model)
- [4. Outcomes: An Open Question](#4-outcomes-an-open-question)
- [5. Type Checking](#5-type-checking)
- [6. Syntax](#6-syntax)
- [7. Questions For The Owner](#7-questions-for-the-owner)
- [Appendix A. Prior Art](#appendix-a-prior-art)
- [Appendix B. Earlier Options And #166](#appendix-b-earlier-options-and-166)
- [Sources](#sources)
- [Parse Log](#parse-log)

## 1. Context And Goal

The goal is to move computation and data between **execution domains**: a
worker, a server, a job that runs later, a GPU, or a thread. Today an hd
program runs in one instance on one thread
([`module.instance.thread`](../../spec/lang/10-modules.md#r-module.instance.thread)).
Instances exchange only boundary-safe values, copied as trees
([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed),
[`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)).
Closures never cross
([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)).

So each program below copies too much, cannot say what it needs, or cannot
be written. The design builds one model of a crossing first. The type
checks follow from the model, and syntax comes last.

## 2. Six Programs That Cross

Each sketch is hd as it is written today, with the crossing it needs in a
comment. Std names such as `Workers`, `Jobs` and `Gpu` are illustrations.

**1. Image editor: a thumbnail in a web worker.** The page sends a 50 MB
photo and the `thumbnail` code to a worker. A copy keeps the page
responsive but costs an encode and a copy. A transfer is fast, but the
page must not touch the photo until it comes back.

```text
struct Image:
    width: u32
    height: u32
    pixels: List[u8]    # 4096 x 3072 RGBA: 50 MB

fn thumbnail(photo: Image, side: u32) -> (Image, Image):
    small := shrink(photo, side)
    (photo, small)

pub fn on_export!(photo: Image) -> Result[Image, WorkerError] $ Workers:
    # Today: the 50 MB list is encoded, copied into the worker and decoded.
    # Wanted: hand the buffer over, and make any use of `photo` an error until it returns.
    let (back, small) = $.use(Workers).call!(thumbnail, photo, 256)?
    .Ok(small)
```

**2. Checkout: the code runs where its row is met.** Only the server has
`Db + Payments`, so `place_order` must run there. The cart is copied there
and the receipt copied back. When the reply times out, the browser cannot
tell whether the order committed.

```text
pub trait Db:
    fn insert_order!(&mut self, cart: Cart) -> Result[OrderId, DbError]

pub trait Payments:
    fn charge!(&mut self, card: CardToken, cents: i64) -> Result[ChargeId, PayError]

pub fn place_order!(cart: Cart) -> Result[Receipt, OrderError] $ Db + Payments:
    charge := $.use(Payments).charge!(cart.card, cart.total_cents)?
    order := $.use(Db).insert_order!(cart)?
    .Ok(Receipt { order, charge })

# Browser code. The browser binds neither Db nor Payments.
fn on_buy!(cart: Cart) -> Result[Receipt, OrderError] $ Server:
    place_order!(cart)  # error today: missing-requirement; wanted: run it on the server
```

**3. Sign-up email: computation moves in time.** A job queue sends the
welcome mail ten minutes later, across a server restart. A newer build may
run it, so the code needs an identity that survives a deploy. The email
address may be captured; a connection or a `&mut` may not.

```text
fn send_welcome!(email: string, name: string) -> Result[void, MailError] $ Mailer:
    $.use(Mailer).send!(email, "Welcome, $name")

pub fn sign_up!(user: User, conn: &mut DbConn) -> Result[void, SignUpError] $ Jobs:
    save_user!(conn, user)?
    # Wanted: a stored job; today a closure is not boundary-safe.
    $.use(Jobs).after!(minutes(10), fn!() -> Result[void, MailError] $ Mailer: send_welcome!(user.email, user.name))?
    $.use(Jobs).after!(minutes(10), fn!() -> Result[void, DbError]: mark_sent!(conn, user.id))?  # wanted: an error, `conn` is a `&mut`
    .Ok(())
```

**4. Photo filter on a GPU: data changes representation.** The photo is
uploaded once, five kernels run on the device, and one image is read
back. On the device the photo is a texture, not a `List[u8]`. The host
holds a handle that means something only on that one device.

```text
pub trait Gpu:
    fn upload!(&mut self, image: Image) -> Result[Texture, GpuError]
    fn run!(&mut self, kernel: fn(&mut Texture) -> void, texture: &mut Texture) -> Result[void, GpuError]
    fn read_back!(&mut self, texture: Texture) -> Result[Image, GpuError]

pub fn retouch!(photo: Image) -> Result[Image, GpuError] $ Gpu:
    gpu := $.use(Gpu)
    let mut tex = gpu.upload!(photo)?    # copied once, rebuilt as a texture
    for kernel in [blur, sharpen, denoise, vignette, tone]:
        gpu.run!(kernel, &mut tex)?      # stays resident on the device
    # Today nothing rejects a kernel that calls println, or `tex` used on a second GPU.
    gpu.read_back!(tex)
```

**5. Parallel word count: borrow, do not copy.** Four threads read
quarters of one big list in place. The scope joins them before it ends,
so the borrow cannot outlive the list. A second pass splits one `&mut`
into two disjoint halves and writes both in parallel.

```text
pub fn word_count(lines: &List[string]) -> Map[string, usize]:
    q := lines.len() / 4
    # Wanted: four threads borrow `lines`; today an instance has no threads.
    counts := scope(fn(s: &mut Scope) -> List[Map[string, usize]]:
        tasks := [for i in 0..4 => s.spawn(fn() -> Map[string, usize]: count_words(lines, i * q, i * q + q))]
        [for t in tasks => t.join()]
    )
    merge_counts(counts)

pub fn normalize(lines: &mut List[string]) -> void:
    let (left, right) = lines.split_halves_mut()    # wanted: two disjoint &mut halves
    scope(fn(s: &mut Scope) -> void:
        s.spawn(fn() -> void: lowercase_all(left))
        lowercase_all(right)
    )
```

**6. Reporting dashboard: code moves to the data.** The browser needs the
200 sales that match a filter. Today it downloads a million rows and
filters locally. The filter code and its small `Filter` value should go
to the server where the rows live.

```text
struct Filter:
    region: string
    since_day: i32
    min_cents: i64

fn matches(f: Filter, sale: Sale) -> bool:
    sale.region == f.region && sale.day >= f.since_day && sale.cents >= f.min_cents

pub fn top_sales!(f: Filter) -> Result[List[Sale], ApiError] $ SalesApi:
    rows := $.use(SalesApi).all_sales!()?          # a million rows cross the network
    .Ok([for s in rows if matches(f, s) => s])     # to keep 200
```

Together they cover space (1, 2, 4, 5, 6) and time (3), the modes copy,
transfer and lend (1, 5), a change of representation (4), an unknown
outcome (2), code identity across builds (3), and code sent to data (6).

## 3. The Model

### Execution Domains

An execution domain has four properties (**approved**):

| Property | Question it answers |
| --- | --- |
| Memory | Is memory shared with the source, or separate? |
| Capabilities | Which providers exist there? |
| Code | Which build runs there? |
| Lifetime | Can it outlive the source? Can contact be lost? |

Five kinds of domain (**approved**):

| Kind | Memory | Capabilities | Code | Lifetime | Example |
| --- | --- | --- | --- | --- | --- |
| Scoped thread | shared | the source's | same build | ends before its scope | 5 |
| Worker | separate | its own host's | same build | may crash | 1 |
| Remote service | separate | the service's | maybe another build | outlives; contact may be lost | 2, 6 |
| Device | separate | restricted operations, kernels only | device code | may reset | 4 |
| Later / resume | separate | the resuming host's | maybe a newer build | outlives the source | 3 |

### A Crossing

A **crossing** sends code and an environment to a destination, and returns
a result.

- **Code** must exist or be identifiable at the destination: a named
  function, or a content hash of a closure's lifted function
  ([#166](https://github.com/HerringtonDarkholme/hd-lang/issues/166)). No
  executable code is uploaded.
- **Environment**: each captured value crosses in one mode, below.
- **Requirement row**: the code's row is checked against the
  destination's capabilities, not the source's. That is example 2.
- **Result**: crosses back by the same modes.

### Crossing Modes

Four modes for environment values (**approved**):

| Mode | The destination gets | The source afterwards | Where |
| --- | --- | --- | --- |
| Copy | its own value | unchanged | all domains |
| Transfer | ownership | loses the value until it is handed back | separate memory |
| Lend | a borrow | frozen or excluded until the borrow ends | shared memory, scoped only |
| Handle | a handle; the data stays where it lives | a handle tied to that one domain instance | all domains |

The owner's note named five modes. Three of them fold into these four:
**share** is lend in shared memory or a handle in separate memory,
**proxy** is a handle, and **reconstruct** is a copy rebuilt in the
destination's representation (example 4's upload).

| Domain | Copy | Transfer | Lend | Handle |
| --- | --- | --- | --- | --- |
| Scoped thread | yes | not needed: an ordinary move | yes | yes |
| Worker | yes | yes | no | yes |
| Remote service | yes | yes | no | yes |
| Device | yes, often rebuilt | yes | no | yes |
| Later / resume | yes | no (L4) | no | yes |

### The Examples In The Model

| # | Source to destination | Code | Environment | Result |
| --- | --- | --- | --- | --- |
| 1 | page to worker | `thumbnail`, by name | `photo`: copy or transfer | thumbnail copied back; a transferred photo handed back |
| 2 | browser to remote service | `place_order`, by name; its row is met only there | `cart`: copy | receipt copied back, or an unknown outcome |
| 3 | now to later | `send_welcome`, by content hash across builds | `email`, `name`: copy; `conn`: rejected | done or failed, with no caller waiting |
| 4 | host to device | five kernels | `photo`: copy rebuilt as a texture; the host keeps a handle | one image copied back |
| 5 | thread to scoped threads | closures, same build | `lines`: shared lend; halves: exclusive lends | counts returned; threads joined before the scope ends |
| 6 | browser to remote service | `matches`, by name | `f`: copy; the rows never move | 200 rows copied back |

### Lifecycle Rules (**approved**)

- **L1.** A borrow ends when the destination's access ends (quiescence),
  not when the result arrives.
- **L2.** An unknown outcome never returns transferred ownership.
- **L3.** A handle keeps its data alive until the last handle is dropped.
- **L4.** The later/resume domain allows only copy and handle.

### Affinity (Open)

Some work belongs near some data (owner, 2026-10-31). Two kinds behave
differently:

| Kind | Meaning | Example | Who decides |
| --- | --- | --- | --- |
| Required | The work can run only where the data is: a handle is valid only in its home domain | five kernels on the GPU that holds the texture (4); a query on the shard that holds the rows (6) | the checker, from the handles the code uses |
| Preferred | The work may run anywhere but is cheaper near data already there | a worker pool reusing the worker that holds the photo (1); a replica with a warm cache | the scheduler at run time, as a cost hint |

A sketch of how each fits the model, for discussion:

- **Required affinity follows from handles.** Every handle has a home
  domain (question 4). A crossing may target the domain a handle lives in,
  instead of a named domain, as Chapel's `on x do` runs where `x` lives.
  The checker then requires every handle the code uses to share that home,
  so a kernel cannot take a texture from another GPU.
- **Preferred affinity is a scheduling hint, not a type.** A domain pool
  (`Workers`) may pick the member that already holds a value. It changes
  cost, never meaning, so the checker need not see it. Prior art keeps it
  in the runtime: Spark and Dask preferred locations, Ray's locality-aware
  scheduling, Orleans placement, NUMA and Kubernetes affinity rules.

## 4. Outcomes: An Open Question

A domain that can lose contact may leave a crossing with no answer. In
example 2 the charge may or may not have happened. The owner asked for
more thought, so this section lays out both options and decides nothing.

**Option 1: unknown is a first-class outcome.** A crossing into a remote
or later domain ends done, failed, or unknown. A worker or device reports
unknown only on a crash or reset. The type system makes callers handle
unknown, and local domains pay nothing. Unknown never returns transfers
and never ends a borrow early (L1, L2).

```text
match $.use(Server).call!(place_order, cart):  # hypothetical syntax
    .Done(receipt) => show(receipt)
    .Failed(error) => show_error(error)
    .Unknown => check_order_status!(cart.id)
```

**Option 2: only done and failed.** A crossing returns `Result[T, E]`,
and each library folds timeouts into its error type. The checker then
cannot tell a timeout from a failure. Under L2 it must treat every error
from a contact-losing domain as "transfers not returned".

| Prior art | What the caller sees | How "unknown" is handled |
| --- | --- | --- |
| gRPC | `DEADLINE_EXCEEDED`, one status among others | Folded into the error; the docs warn the operation may have completed |
| Temporal | An activity times out and is retried; the workflow sees one completion | The runtime retries; activities should be idempotent |
| Azure Durable Functions | Orchestrators replay from a history | Activities may run more than once; orchestrator code must be deterministic |
| Erlang monitors | `{'DOWN', Ref, process, Pid, noconnection}` | A distinct reason: the process may still exist |
| Swift distributed actors | Every remote call is implicitly `async throws` | Transport failures are errors, forced on remote calls only |
| Stripe idempotency keys | A retry with the same key returns the first result | The client resolves unknown by retrying safely |

gRPC and Swift are option 2, with docs or types forcing attention.
Erlang is option 1 at the process level. Temporal, Durable Functions and
Stripe turn unknown into "retry until known", which needs idempotent work.

## 5. Type Checking

The checker verifies each crossing against the model. It reuses hd's
rules where they exist, and the last column marks each gap.

| Check | Rule it reuses | Gap |
| --- | --- | --- |
| The code exists at the destination: a name, or a content hash for another build | [`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program) | No content identity yet ([`fn.capture.serializable-deferred`](../../spec/lang/07-functions.md#r-fn.capture.serializable-deferred)) |
| The destination's capabilities meet the code's row | [`req.row.set.call`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.set.call), [`module.register.row`](../../spec/lang/10-modules.md#r-module.register.row); providers are never captured ([`fn.capture.no-providers`](../../spec/lang/07-functions.md#r-fn.capture.no-providers)) | Domains do not declare capabilities yet |
| Each capture's mode is allowed for the domain kind | The domain-by-mode table | A new rule |
| Copy: the type is boundary-safe in separate memory | [`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed), [`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe) | None for plain values |
| Transfer: the source owns the value; the crossing ends the binding as an M1 move does | [Moves](https://github.com/HerringtonDarkholme/hd-lang/blob/cc862252e/spec/lang/13-values-and-references.md#moves): `ref.move.use` gives `use-of-moved-value`; `ref.move.reassign` restores the binding | Only `&mut` moves; other values copy until `~Copyable` ([#535](https://github.com/HerringtonDarkholme/hd-lang/issues/535), [#1138](https://github.com/HerringtonDarkholme/hd-lang/issues/1138)) |
| Lend: only to a scoped shared-memory domain, joined before the scope ends | A closure argument has a fresh lifetime ([`fn.capture.local-parameter`](../../spec/lang/07-functions.md#r-fn.capture.local-parameter)) | No threads, no shared heap, no `Send` or `Sync` |
| Lend `&`: the source does not write while lent | [`ref.excl.shared`](../../spec/lang/13-values-and-references.md#r-ref.excl.shared) | Run-time loans are per instance ([`ref.excl.alias.conflict`](../../spec/lang/13-values-and-references.md#r-ref.excl.alias.conflict)) |
| Lend `&mut`: the source is excluded; disjoint parts lend separately | [`ref.excl.mut`](../../spec/lang/13-values-and-references.md#r-ref.excl.mut); [Result Paths](../../spec/lang/13-values-and-references.md#result-paths) for fields | Disjoint list halves need a slice type |
| A lending scope does not suspend | A borrowing suspension never reaches a host call ([`ref.dep.suspension.store`](../../spec/lang/13-values-and-references.md#r-ref.dep.suspension.store)); `abandon` unwinds synchronously ([`req.abandon.unwind`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.unwind)) | None: L1 matches today's strictest rule |
| A stored or later job is `Fn` and captures no `&` or `&mut` | [`fn.trait.read-only`](../../spec/lang/07-functions.md#r-fn.trait.read-only), [`ref.local.escape.capture-stored`](../../spec/lang/13-values-and-references.md#r-ref.local.escape.capture-stored) | None |
| A handle's type carries its domain instance | Type parameters, as `Texture[D]` | Static instances only; a run-time instance needs a check |
| A handle is released at scope exit | `using` and `@auto_closing` ([`flow.auto-closing.statement`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.statement)) | L3 needs counting across domains |
| An abandoned crossing aborts the host operation | [`req.abandon.host`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.host) | After a transfer, the outcome is unknown (section 4) |

What the checker would reject, in the model's terms:

```text
on_worker(transfer(photo), fn() -> Image: thumbnail(photo, 256))  # hypothetical syntax
show(photo)                     # hypothetical syntax; error: use-of-moved-value, photo is in the worker
on_worker(lend(&lines), count)  # hypothetical syntax; error: lend needs a scoped shared-memory domain
later(copy(conn), mark_sent)    # hypothetical syntax; error: a &mut is not boundary-safe
gpu1.run!(blur, &mut tex)       # hypothetical syntax; error: a Texture[Gpu0] used on Gpu1
```

Four gaps block whole modes: no `Send` or `Sync`, no threads in one heap,
no shared heap on Wasm, and no ownership for plain values. Copy and handle
need none of them.

## 6. Syntax

Every spelling here is hypothetical and needs approval. Two alternatives:

```text
small := $.use(Workers).call!(thumbnail, transfer(photo), 256)?                   # hypothetical syntax
small := at workers capture { photo = transfer(photo) } do: thumbnail(photo, 256)  # hypothetical syntax
```

The first is a call on a domain value, with a mode written on each
argument; copy is the default and stays unwritten. The second is the
owner's note's block, which names every capture and its mode.

## 7. Questions For The Owner

1. **Is "unknown" a first-class outcome?** Option 1: done, failed or
   unknown from remote and later domains, enforced by types. Option 2: only
   done and failed; libraries fold timeouts into errors. No recommendation
   (section 4).
2. **Do L1 to L4 hold?** Answered: yes, all four (owner, 2026-10-31).
3. **Does code name the destination?** Answered (owner, 2026-10-31): a
   domain value in code for workers and devices, a provider such as `Gpu`
   or `Workers` from the requirement row (swappable with `$.with` in
   tests); services stay named by the deployment configuration, as the
   2026-10-17 #166 decision says.
4. **How does a handle name its domain instance?** Options: (a) in the
   type, as `Texture[Gpu0]`, for static instances only; (b) a run-time
   instance id checked on use, panicking on a mismatch. Recommended: (a),
   adding (b) when instances are dynamic.
5. **When does transfer apply to plain values?** Only `&mut` moves today.
   Options: (a) transfer waits for `~Copyable` (#535); (b) transfer now for
   `&mut` and handle types only. Recommended: (a).
6. **How is affinity expressed?** Required affinity: (a) a crossing may
   target a handle's home domain, and the checker requires every handle in
   the code to share it; (b) code always names the domain, and a handle
   from another domain is a type error. Preferred affinity: a run-time hint
   to a domain pool, or left to deployment. Decide with question 4, since
   required affinity reads the handle's domain.

```text
let tex: Texture[Gpu0] = gpu0.upload!(photo)?  # question 4 (a)
```

## Appendix A. Prior Art

Each snippet is abridged from its source.

**Rust scoped threads.** Threads borrow from the caller; the scope joins
them before it returns. hd takes this as the lend mode and L1.

```rust
let (left, right) = words.split_at(words.len() / 2);
let total = std::thread::scope(|s| {
    let a = s.spawn(|| count(left));
    count(right) + a.join().unwrap()
});
```

**Swift `sending` (SE-0430).** A value whose region nothing else reaches
moves to another isolation domain, and a later use is an error. hd takes
this as transfer, checked as an M1 move.

```swift
func process(_ image: sending Image) async { /* ... */ }
let img = Image()
await process(img)
// a use of img here is an error: it was sent
```

**Chapel `on`.** An `on` clause moves the running task, and `on x` runs
where `x` lives. hd takes "code moves to the data" (example 6).

```chapel
on Locales[1] do writeln(here.id);
on A[i] do A[i] += 1;
```

**ML5.** Types name the world a value lives at, and `get` evaluates an
expression at another world. hd takes the domain-tagged handle.

```sml
from server get (lookup key)
```

**ScalaLoci.** Peers and their ties are types, a placed value is read
remotely with `asLocal`, and transmitters may change the received type.
hd takes reconstruct and handle; its coherence rules make ScalaLoci's
endpoint check unnecessary.

```scala
@peer type Server <: { type Tie <: Multiple[Client] }
val items: Items on Server = /* ... */
on[Client] { Signal { items.asLocal() map createUIEntry } }
```

**Spores.** A closure whose captures are listed and checked, so it can be
sent. hd takes checked captures, one mode each.

```scala
val s = spore {
  val h = helper
  (x: Int) => h.inc(x)
}
```

**Unison.** Code is identified by content hash, and `Value.map` moves a
function to a remote value. hd takes the content hash for example 3.

```unison
Seq (Value.map go (Seq.unwrap s))
```

## Appendix B. Earlier Options And #166

The brainstorm compared four options. The model keeps a part of each.

| Option | Kept | Dropped |
| --- | --- | --- |
| A: named code, copied data | Named functions as code identity; the copy mode | Copy as the only mode |
| B: portable closures placed by rows (#166) | Content hash across builds; rows checked at the destination | Placement by configuration alone (question 3) |
| C: typed crossing contracts (owner's note) | A mode per capture; a destination representation; handles tied to an instance | A user `Across` trait per type as the primary check |
| D: `Send`, `Sync`, scoped threads | The scoped-thread domain and lend | Markers now: they wait for threads |

The parked #166 study
([SERIALIZABLE_CLOSURE_DESIGN.md @ 28072b8a1](https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md))
is the copy-mode slice of this model.

| #166 | This model |
| --- | --- |
| `@remote` functions and portable closure literals | Code identity: a name or a content hash |
| Captures must be boundary-safe | The copy mode's check |
| Placement by requirement rows and `[places]` | The row is checked against a domain's capabilities |
| `Opaque[T]` for server state | A handle |
| `RemoteError` with `OutcomeUnknown` | Option 1 of section 4, inside an error type |
| Workers, servers, services; GPUs deferred | All five domain kinds; lend and transfer added |

## Sources

- The owner's note, "Types and Transport Across Execution Boundaries" (gist, 2026-10-31), and the owner's review of PR #1139.
- #166 study: <https://github.com/HerringtonDarkholme/hd-lang/blob/28072b8a1/docs/future-work/SERIALIZABLE_CLOSURE_DESIGN.md>; [Parallelism And Locality](PARALLELISM_AND_LOCALITY.md).
- gRPC status codes, `DEADLINE_EXCEEDED`: <https://grpc.github.io/grpc/core/md_doc_statuscodes.html>
- Temporal, activity timeouts: <https://docs.temporal.io/encyclopedia/detecting-activity-failures>; idempotency: <https://docs.temporal.io/activity-definition>
- Azure Durable Functions, orchestrator code constraints: <https://learn.microsoft.com/en-us/azure/azure-functions/durable/durable-functions-code-constraints>
- Erlang, `monitor/2` and `noconnection`: <https://www.erlang.org/doc/apps/erts/erlang.html#monitor/2>
- Swift SE-0336, distributed actor isolation: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0336-distributed-actor-isolation.md>
- Stripe, idempotent requests: <https://docs.stripe.com/api/idempotent_requests>
- Rust, `thread::scope`: <https://doc.rust-lang.org/std/thread/fn.scope.html>
- Swift SE-0430, `sending`: <https://github.com/swiftlang/swift-evolution/blob/main/proposals/0430-transferring-parameters-and-results.md>
- Chapel, on clauses: <https://chapel-lang.org/docs/users-guide/locality/onClauses.html>
- Murphy, Crary, Harper, "Type-Safe Distributed Programming with ML5", TGC 2007: <https://kilthub.cmu.edu/articles/journal_contribution/Type-Safe_Distributed_Programming_with_ML5/6611192/1>
- ScalaLoci: <https://scala-loci.github.io/>; Weisenburger, Salvaneschi, Programming 2020: <https://scala-loci.github.io/publications/2020-programming/paper.pdf>
- Miller, Haller, Odersky, "Spores", ECOOP 2014: <https://doi.org/10.1007/978-3-662-44202-9_13>
- Unison, distributed datasets: <https://www.unison-lang.org/articles/distributed-datasets/>

## Parse Log

All 10 `text` blocks were parsed on 2026-10-31 with
`scripts/hd-conformance.sh parse`, which checks syntax only; nothing was
type-checked. Std names (`Workers`, `Jobs`, `Gpu`, `scope`) are sketches.

| Block | Where | Result |
| --- | --- | --- |
| 1 to 6 | Examples 1 to 6 | parse |
| 7 | Section 4, option 1 | parses; the `.Done`/`.Unknown` outcome is hypothetical |
| 8 | Section 5, rejected crossings | parses; every line is `# hypothetical syntax` in meaning |
| 9 | Section 6, syntax | fails as expected at `at workers`: both lines `# hypothetical syntax` |
| 10 | Question 4 | parses |
