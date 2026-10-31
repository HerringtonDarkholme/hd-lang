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
- [4. Outcomes](#4-outcomes-approved-question-1)
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

### Affinity (**approved**: question 6)

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

## 4. Outcomes (**approved**: question 1)

A domain that can lose contact may leave a crossing with no answer. In
example 2 the charge may or may not have happened. The owner decided
(2026-10-31): there is no new outcome type. A crossing returns an ordinary
`Result[T, E]`, and unknown is one case of its error type: a std crossing
error with an `Unknown` case beside the domain's own errors. The caller
handles it with an ordinary `match`. A worker or device reports unknown
only on a crash or reset, and a scoped thread never does.

```text
match place_order!(cart):  # hypothetical syntax: placed on the server by the deployment configuration
    .Ok(receipt) => show(receipt)
    .Err(.Unknown) => check_order_status!(cart.id)
    .Err(error) => show_error(error)
```

L1 and L2 hold by construction, so the checker needs no rule for unknown
(section 5, Outcomes And Cancellation).

| Prior art | What the caller sees | How "unknown" is handled |
| --- | --- | --- |
| gRPC | `DEADLINE_EXCEEDED`, one status among others | Folded into the error; the docs warn the operation may have completed |
| Temporal | An activity times out and is retried; the workflow sees one completion | The runtime retries; activities should be idempotent |
| Azure Durable Functions | Orchestrators replay from a history | Activities may run more than once; orchestrator code must be deterministic |
| Erlang monitors | `{'DOWN', Ref, process, Pid, noconnection}` | A distinct reason: the process may still exist |
| Swift distributed actors | Every remote call is implicitly `async throws` | Transport failures are errors, forced on remote calls only |
| Stripe idempotency keys | A retry with the same key returns the first result | The client resolves unknown by retrying safely |

hd follows gRPC, Erlang and Temporal: unknown is a case the caller
matches, not a separate type. Temporal, Durable Functions and Stripe then
turn unknown into "retry until known", which needs idempotent work; a
library can build that on the `Unknown` case.

## 5. Type Checking

The checker sees a crossing as a call with three inputs: a destination,
code, and an environment. It checks them at four constructs: the
crossing, each capture, each use of a handle, and the code's body. Each
check names what it enforces from section 3, the hd rules it reuses, and
what is new. A table at the end sums up.

The destination comes from code or from the deployment (question 3).
Workers and devices are provider values from the source's row, so a test
swaps them with `$.with`. A remote service is named by the deployment
configuration. A crossing may also target a handle's home, read from the
handle's type (question 6). The sketches assume these std traits:

```text
# hypothetical syntax throughout this section
trait Workers[$Cap]:    # $Cap: the capabilities a worker offers
    fn call![T, F < Fn() -> T $Cap](&mut self, code: F) -> Result[T, CrossingError]

trait Gpu:
    type Device         # one type per device, as Gpu0
    fn upload!(&mut self, image: Image) -> Result[Texture[Self::Device], CrossingError]
    fn run!(&mut self, kernel: fn(Texture[Self::Device]) -> void, texture: Texture[Self::Device]) -> Result[void, CrossingError]
```

### The Row Against The Destination

- **Checked at the crossing:** every key in the code's row is a capability
  of the destination. The source's row need not hold them (example 2).
- **Enforces:** the capabilities property; the row is checked at the
  destination, not the source (A Crossing).
- **Reuses:** a domain trait carries its capabilities as a row parameter,
  as `Fn[I, O, $R]` does ([`fn.trait.decl`](../../spec/lang/07-functions.md#r-fn.trait.decl)). Code whose row lists a key
  the destination lacks does not fit, `type-mismatch`
  ([`req.row.subsume.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.missing)); a narrower row fits
  ([`req.row.subsume.closure`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.subsume.closure)). A closure captures no provider from
  `$.with`, so its row lists every key its body uses
  ([`fn.capture.no-providers`](../../spec/lang/07-functions.md#r-fn.capture.no-providers)). Two devices are two keys,
  `Gpu[Device = Gpu0]` and `Gpu[Device = Gpu1]`
  ([`req.key.binding.identity`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.binding.identity)). A service's registered bindable set
  stands for its capabilities ([`module.register.row`](../../spec/lang/10-modules.md#r-module.register.row)).
- **New:** each domain trait declares its capabilities, and the checker
  reads a service's set from the deployment configuration. A call that the
  configuration places on a service is checked against that set, not
  against the source's row ([`req.row.set.call`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.set.call)).

```text
# hypothetical syntax
workers.call!(fn() -> void $ Db: save(doc))?                # error: type-mismatch, Db is not a worker capability
gpu.run!(fn(t: Texture[Gpu0]) -> void: println("x"), tex)?  # error: type-mismatch, a kernel's row is empty
workers.call!(fn() -> void $ Log: log_size(doc))?           # valid: the worker offers Log
```

### Each Capture's Mode

- **Checked at each capture, argument and result:** the value crosses in a
  mode the domain kind allows. This is the domain-by-mode table, with L4
  as its last row.
- **Reuses:** capture analysis finds the captures ([`fn.capture.locals`](../../spec/lang/07-functions.md#r-fn.capture.locals)).
  The move checker's copy test sorts them: a `&mut` and a type holding one
  are not copied ([`ref.move.mut`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.mut), [`ref.move.holds`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.holds)).
- **New:** the table as a rule. The mode follows from the value's type and
  the domain kind. A copyable value copies, a non-copyable one transfers, a
  `&` or `&mut` lends, and a handle type crosses as a handle. So the
  checker infers every mode, and a written mode is a claim it verifies.

The four modes add these checks.

**Copy**, at each copied capture, argument and result. In separate
memory, the value must be boundary-safe. In shared memory, any copyable
value copies, as a capture does today ([`fn.capture.copy.value`](../../spec/lang/07-functions.md#r-fn.capture.copy.value)).

- **Enforces:** the destination gets its own value; the source is
  unchanged.
- **Reuses:** boundary-safe values ([`module.boundary.allowed`](../../spec/lang/10-modules.md#r-module.boundary.allowed)): closures,
  trait values, handles and references fail
  ([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)). Values cross as trees
  ([`module.boundary.tree`](../../spec/lang/10-modules.md#r-module.boundary.tree)), and private fields need `Serialize`
  ([`module.boundary.out`](../../spec/lang/10-modules.md#r-module.boundary.out), [`module.boundary.error`](../../spec/lang/10-modules.md#r-module.boundary.error)).
- **New:** the boundary check runs on a crossing's captures, not only on a
  registered signature. Code for separate memory is bounded `Fn`, so a
  closure that writes a capture is `not-writable`
  ([`fn.trait.read-only`](../../spec/lang/07-functions.md#r-fn.trait.read-only)). Today a captured variable shares a cell with
  its scope ([`fn.capture.cell.writes`](../../spec/lang/07-functions.md#r-fn.capture.cell.writes)); across memories the write would
  change only the destination's copy.

```text
# hypothetical syntax
workers.call!(fn() -> Image: shrink(photo, edits))?  # error: edits is a *mut Edits, not boundary-safe
workers.call!(fn() -> void: done += 1)?              # error: not-writable, the write would change only the worker's copy
workers.call!(fn() -> Image: shrink(photo, 256))?    # valid: Image is boundary-safe, so it is copied
```

**Transfer**, at the capture and at each later use of the source binding.
The value's type is not copyable, and the crossing ends the source binding
as a move.

- **Enforces:** the destination gets ownership; the source loses the value
  until it is handed back. Transfer is for non-copyable types only
  (question 5).
- **Reuses:** the moves of M1 and M2 ([`ref.move.mut`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.mut),
  [`ref.move.holds`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.holds)). A later use is `use-of-moved-value`
  ([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)); assigning the returned value makes the binding
  usable again ([`ref.move.reassign`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.reassign)).
- **New:** a crossing's capture is a move site, one more row of the
  [`ref.move.sites`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.sites) table. Transfer of a copyable type is an error; the
  value crosses as a copy. In separate memory the value must also hold no
  reference into the source's memory. Every non-copyable type today holds
  a `&mut`, so transfer into a worker, a service or a device has no valid
  case before `~Copyable`
  ([#535](https://github.com/HerringtonDarkholme/hd-lang/issues/535)).
  Until then transfer is the scoped thread's ordinary move.

```text
# hypothetical syntax
workers.call!(fn() -> (Image, Image): thumbnail(transfer(photo), 256))?  # error: Image is copyable, so photo is copied
scope(fn(s: &mut Scope) -> void:
    s.spawn(fn() -> void: lowercase_all(left))  # left, a SpanMut, moves into the thread
    lowercase_all(left)                          # error: use-of-moved-value
    lowercase_all(right)                         # valid: right never moved
)
```

**Lend**, at the capture and at the scope call that joins. A `&` or
`&mut` capture goes only to a scoped shared-memory domain, and the loan
stays live until that domain's access ends (L1).

- **Enforces:** the source is frozen or excluded until the borrow ends,
  and the borrow ends at quiescence.
- **Reuses:** a closure given directly for a `fn` parameter may capture
  local references ([`fn.capture.local-parameter`](../../spec/lang/07-functions.md#r-fn.capture.local-parameter)); storing that
  parameter is an error ([`fn.capture.local-store`](../../spec/lang/07-functions.md#r-fn.capture.local-store)). The scope keeps its
  spawned closures under a lifetime bound, `F < FnMut() -> T & 's`
  ([`ref.generic.outlives`](../../spec/lang/13-values-and-references.md#r-ref.generic.outlives)). The loan lasts for the scope call
  ([`ref.excl.call`](../../spec/lang/13-values-and-references.md#r-ref.excl.call), [`ref.excl.live`](../../spec/lang/13-values-and-references.md#r-ref.excl.live)). While `&p` is lent the source
  does not write `p` ([`ref.excl.shared`](../../spec/lang/13-values-and-references.md#r-ref.excl.shared)); while `&mut p` is lent it does
  not use `p` ([`ref.excl.mut`](../../spec/lang/13-values-and-references.md#r-ref.excl.mut)). Disjoint fields lend separately
  ([`ref.excl.place`](../../spec/lang/13-values-and-references.md#r-ref.excl.place); through a call, [`ref.result.path.caller`](../../spec/lang/13-values-and-references.md#r-ref.result.path.caller)).
  Separate-memory code is bounded `'static`, so a local-reference capture
  there is `local-reference-escapes` ([`ref.generic.outlives.box`](../../spec/lang/13-values-and-references.md#r-ref.generic.outlives.box),
  [`ref.local.escape.capture-stored`](../../spec/lang/13-values-and-references.md#r-ref.local.escape.capture-stored)). A suspension that holds a local
  reference never reaches a host call ([`ref.dep.suspension.store`](../../spec/lang/13-values-and-references.md#r-ref.dep.suspension.store)). So
  a lending scope is an ordinary call that returns after the join, and
  nothing abandons it midway: L1 needs no new rule.
- **New:** the scope itself, which needs threads. The run-time loans that
  guard counted references are per instance
  ([`ref.excl.alias.conflict`](../../spec/lang/13-values-and-references.md#r-ref.excl.alias.conflict)) and would have to see other threads. A
  shared lend is not deep: a read through `&T` may write through a `*mut`
  field ([`ref.read.shallow`](../../spec/lang/13-values-and-references.md#r-ref.read.shallow)), and a captured variable is a shared cell
  ([`fn.capture.cell`](../../spec/lang/07-functions.md#r-fn.capture.cell)). Both race across threads; that is the missing
  `Sync`.

```text
# hypothetical syntax; lines: &mut List[string]
workers.call!(fn() -> usize: count_words(lines))?  # error: local-reference-escapes, a worker has separate memory
counts := scope(fn(s: &mut Scope) -> List[Map[string, usize]]:
    tasks := [for i in 0..4 => s.spawn(fn() -> Map[string, usize]: count_quarter(lines, i))]
    lines.push("")                  # error: conflicting-access, lines is lent until the scope joins
    [for t in tasks => t.join()]
)
lines.push("")                      # valid: the scope has joined
```

**Handle**, at each call that takes the handle. A handle's type names its
home domain, so using it in another domain is a type error, with no
run-time check (question 4).

- **Enforces:** the handle row of the mode table, and L3.
- **Reuses:** the device trait's associated type, bound in its key:
  `$.use(Gpu[Device = Gpu0])` has type `&dyn Gpu[Device = Gpu0]`
  ([`req.key.binding.value.reference-values`](../../spec/lang/11-requirements-and-suspension.md#r-req.key.binding.value.reference-values)). Its `upload!` returns a
  `Texture[Gpu0]`, and its `run!` takes only that, so a `Texture[Gpu0]`
  given to Gpu1 is an ordinary `type-mismatch`. Handles are not
  boundary-safe ([`module.boundary.not-safe`](../../spec/lang/10-modules.md#r-module.boundary.not-safe)), so a copy never takes one.
  Within a domain a handle keeps its data alive as a counted reference
  does ([`ref.box.lifetime`](../../spec/lang/13-values-and-references.md#r-ref.box.lifetime)). `using` or an `@auto_closing` constructor
  releases it at scope exit ([`flow.auto-closing.statement`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.statement)).
- **New:** a compiler-known marker for handle types, as `AutoClosing` is
  one ([`flow.auto-closing.marker`](../../spec/lang/06-control-flow.md#r-flow.auto-closing.marker)). The checker needs it to pick the
  handle mode and to find handles for affinity. Counting a handle across
  domains (L3) is run-time work. Domains are static: one picked at run
  time would need a fresh type (question 4).

```text
# hypothetical syntax
let tex = $.use(Gpu[Device = Gpu0]).upload!(photo)?  # tex: Texture[Gpu0]
$.use(Gpu[Device = Gpu1]).run!(blur, tex)?           # error: type-mismatch, a Texture[Gpu0] on Gpu1
$.use(Gpu[Device = Gpu0]).run!(blur, tex)?           # valid
```

### Handle Agreement And The Affinity Target

- **Checked at the crossing's target and each handle the code uses:** a
  crossing aimed at a handle runs in the domain the handle's type names,
  and every handle the code captures or takes names that same domain.
- **Enforces:** required affinity (question 6), as Chapel's `on x do`.
  Preferred affinity is a run-time hint to a pool such as `Workers`; the
  checker never sees it.
- **Reuses:** the home is a type argument, so "the same domain" is type
  equality. The target's capabilities come from its key, checked as in the
  row check; a row without that key is `missing-requirement`
  ([`req.use.missing`](../../spec/lang/11-requirements-and-suspension.md#r-req.use.missing)).
- **New:** a target read from a type: `at(tex)` with `tex: Texture[D]`
  resolves to the provider for `Gpu[Device = D]`. Then a walk compares
  every handle in the code with `D`.

```text
# hypothetical syntax
at(tex).call!(fn() -> void: blend(tex, mask))?  # error: mask is a Texture[Gpu1], the target is Gpu0
at(tex).call!(fn() -> void: blur(tex))?         # valid: runs on Gpu0, where tex lives
```

### The Later Domain (L4)

- **Checked at each capture of a job and at its code:** a job for later
  takes only copies and handles, and its code has an identity that
  survives a deploy.
- **Enforces:** L4, and the code property: a newer build may run the job.
- **Reuses:** the copy check rejects a reference, which is not
  boundary-safe, and the `'static` bound rejects a lend
  ([`ref.generic.outlives.box`](../../spec/lang/13-values-and-references.md#r-ref.generic.outlives.box)). The table's L4 row rejects a
  non-copyable value. The job's row is checked against the resuming host's
  capabilities, as in the row check.
- **New:** no check beyond the table row. Code identity across builds is a
  gap: a named function is the same only within one compiled program
  ([`req.determinism.same-program`](../../spec/lang/11-requirements-and-suspension.md#r-req.determinism.same-program)), and content hashes are deferred
  ([`fn.capture.serializable-deferred`](../../spec/lang/07-functions.md#r-fn.capture.serializable-deferred)).

```text
# hypothetical syntax
jobs.after!(minutes(10), fn() -> Result[void, DbError] $ Db: mark_sent(conn, user.id))?  # error: conn is a &mut; later takes only copies and handles
jobs.after!(minutes(10), fn() -> Result[void, MailError] $ Mailer: send_welcome(user.email, user.name))?  # valid: two strings are copied
```

### The Body Against The Destination's Environment

- **Checked at the body of the crossing's code:** it runs with what the
  destination gives it: its parameters, its captures as they crossed, and
  the destination's providers for its row. It sees no provider and no
  memory of the source.
- **Enforces:** the capabilities and memory properties.
- **Reuses:** a body's available keys are its row plus lexical keys
  ([`req.row.entail.available`](../../spec/lang/11-requirements-and-suspension.md#r-req.row.entail.available)). A closure's keys resolve at each call
  ([`req.with.dynamic`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.dynamic)), so here they resolve at the destination. A
  provider value captured from `$.use` would fix the source's provider
  ([`req.with.lexical`](../../spec/lang/11-requirements-and-suspension.md#r-req.with.lexical), [`fn.capture.provider-value`](../../spec/lang/07-functions.md#r-fn.capture.provider-value)). It is a counted
  value, not boundary-safe, so the copy check rejects it.
- **New:** nothing for the row. A device body must be device code, a
  restricted subset with no allocation and no host calls. That subset is
  a new check, not specified here.

```text
# hypothetical syntax
log := $.use(Log)
workers.call!(fn() -> void: log.info("done"))?               # error: the source's Log provider is not boundary-safe
workers.call!(fn() -> void $ Log: $.use(Log).info("done"))?  # valid: the worker's own Log
```

### Outcomes And Cancellation

- **Checked:** nothing specific to unknown. A crossing returns
  `Result[T, E]`, and unknown is one case of `E`, handled with `match`
  (section 4). The checker treats `.Unknown` as any other error case, so
  section 4's `match` is the only sketch.
- **Why L1 needs no rule:** a lend goes only to a scoped shared-memory
  domain, which always joins and never reports unknown.
- **Why L2 needs no rule:** a transfer is a move, so the source binding is
  gone already ([`ref.move.use`](https://github.com/HerringtonDarkholme/hd-lang/blob/78c1ee262/spec/lang/13-values-and-references.md#r-ref.move.use)). A transferred value comes back only
  inside `Ok`. A handle creation with an unknown outcome returns no
  handle.
- **Cancellation reuses:** abandoning a crossing aborts the host operation
  as that operation's rule says ([`req.abandon.host`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.host)) and unwinds once,
  synchronously ([`req.abandon.unwind`](../../spec/lang/11-requirements-and-suspension.md#r-req.abandon.unwind)). A moved value stays moved, and
  abandoning closes no handle.

### What Is New

| Check | At | Reused | New |
| --- | --- | --- | --- |
| Row against the destination | crossing | row subsumption, bound keys, no captured providers, bindable sets | capabilities on domain traits; a service's set from the configuration |
| Mode per capture | capture | capture analysis; the copy test of M1 and M2 | the kind-by-mode rule; modes inferred from types |
| Copy | capture, result | boundary-safe values, `Fn` bounds | the boundary check at captures; `Fn` for separate memory |
| Transfer | capture, later uses | M1 and M2 moves | a capture as a move site; no separate-memory case before `~Copyable` |
| Lend | capture, scope call | closure-parameter lifetimes, lifetime bounds, exclusivity, the suspension store rule | the scope; run-time loans across threads |
| Handle | handle use | bound keys, type equality | a handle marker; counts across domains |
| Affinity target | crossing | type equality, `missing-requirement` | a target read from a type; the handle walk |
| Later (L4) | capture, code | the copy check, the `'static` bound | none |
| Body | body | row resolution at each call, the copy check | the device-code subset |
| Outcomes | none | `Result`, `match`, moves, `abandon` | none |

The gaps:

- **No `Send` or `Sync`.** A shallow `&` and a shared cell race across
  threads.
- **No threads and no shared heap.** One instance runs on one thread
  ([`module.instance.thread`](../../spec/lang/10-modules.md#r-module.instance.thread)), and instances' heaps are disjoint
  ([`module.instance.disjoint`](../../spec/lang/10-modules.md#r-module.instance.disjoint)). Lend waits for both.
- **No content identity.** A job that a newer build runs, or a service on
  another build, needs it
  ([`fn.capture.serializable-deferred`](../../spec/lang/07-functions.md#r-fn.capture.serializable-deferred)).
- **No owned non-copyable type.** Transfer into separate memory waits for
  `~Copyable` ([#535](https://github.com/HerringtonDarkholme/hd-lang/issues/535)).
- **No device-code subset.** A kernel's body is checked only by its row.
- **The outcome type is not a gap.** Unknown is an error case (section 4).

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

1. **Is "unknown" a first-class outcome?** Answered (owner, 2026-10-31):
   no new type. A crossing returns `Result[T, E]`, and unknown is an
   `Unknown` case of a std crossing error, matched like any other error
   (section 4).
2. **Do L1 to L4 hold?** Answered: yes, all four (owner, 2026-10-31).
3. **Does code name the destination?** Answered (owner, 2026-10-31): a
   domain value in code for workers and devices, a provider such as `Gpu`
   or `Workers` from the requirement row (swappable with `$.with` in
   tests); services stay named by the deployment configuration, as the
   2026-10-17 #166 decision says.
4. **How does a handle name its domain instance?** Answered (owner,
   2026-10-31): in the type, as `Texture[Gpu0]`; using a handle in another
   domain is a type error, with no run-time check. Dynamic instances (a
   worker picked at run time) would need a fresh type per domain value, as
   Rust's generativity does; not planned now.
5. **When does transfer apply to plain values?** Answered (owner,
   2026-10-31): transfer is for non-copyable types only. Today those all
   hold a `&mut` into the source's memory, so transfer to separate memory
   has no valid case yet; the owner moved `~Copyable` forward (its own
   design) so a user type such as a photo buffer can opt out of copying and
   transfer (example 1).
6. **How is affinity expressed?** Answered (owner, 2026-10-31): required
   affinity, (a): a crossing may target a handle's home domain, read from
   its type, and the checker requires every handle the code uses to share
   it, as Chapel's `on x do`. Preferred affinity is a run-time hint to a
   domain pool (`Workers`), outside the checker.

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
| `RemoteError` with `OutcomeUnknown` | The `Unknown` case of the crossing error (section 4) |
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

All 18 `text` blocks were parsed on 2026-10-31 with
`scripts/hd-conformance.sh parse`, which checks syntax only; nothing was
type-checked. Std names (`Workers`, `Jobs`, `Gpu`, `scope`) are sketches.

| Block | Where | Result |
| --- | --- | --- |
| 1 to 6 | Examples 1 to 6 | parse |
| 7 | Section 4, the `match` on a crossing's result | parses; the placement and the `.Unknown` case are hypothetical |
| 8 | Section 5, the assumed `Workers` and `Gpu` traits | parses; `# hypothetical syntax` |
| 9 to 16 | Section 5, one invalid and one valid sketch per check | parse; every block is `# hypothetical syntax` |
| 17 | Section 6, syntax | fails as expected at `at workers`: both lines `# hypothetical syntax` |
| 18 | Question 4 | parses |
